//! Reference receivers (spec 07 "Methods And Receivers"): a method declared
//! `&self` takes `&Self`, one declared `&mut self` takes `&mut Self`, and a
//! call of one on a place takes the place's address, as Go does:
//! `p.translate(d)` means `(&mut p).translate(d)` (`fn.method.auto-address`).
//! A call on a place needs it writable for `&mut self`; either reference
//! receiver on a temporary refers to a new object holding its value.
//!
//! In the checker's intermediate closure form the address is a `std.core.Ref[T]`
//! (`crates/hd_wasm_linear/DESIGN.md` §15.2, "References"), built by `refs.rs`.

use hd_base::{DefId, LocalId, NodeIdx, StageResult};
use hd_diag::{Code, FixSafety};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{ChoiceKind, NONE, Ref, Tag, TirSink, local_flags};
use hd_types::{Ty, TyData};

use crate::body::Ck;
use crate::call::Hit;
use crate::refs::{Operand, Place, Root};

impl Ck<'_, '_> {
    /// Whether the method `method`, reached with the impl choice `choice`,
    /// takes a reference receiver, and whether it is `&mut`: its declared
    /// receiver is `&self` or `&mut self`.
    pub(crate) fn ref_receiver_of(&self, method: DefId) -> StageResult<Option<bool>> {
        let pool = self.pool();
        let sig = self.sig_of(method)?;
        let Some(&(s, st)) = sig.params.first() else {
            return Ok(None);
        };
        if self.cx.names.text(s) != "self" {
            return Ok(None);
        }
        if let TyData::Ref { mutable, .. } = pool.get(st) {
            return Ok(Some(mutable));
        }
        Ok(None)
    }

    /// The type a place-shaped receiver expression would read, without
    /// evaluating it: a local or top-level binding, a field of one, or a
    /// value read through a reference (`*r`). `None` for any other
    /// expression.
    pub(crate) fn peek_place_ty(&self, e: NodeRef<'_>) -> Option<Ty> {
        let kids: Vec<NodeRef<'_>> = e.children().collect();
        match e.kind() {
            SyntaxKind::ParenExpr => match kids.as_slice() {
                [inner] => self.peek_place_ty(*inner),
                _ => None,
            },
            SyntaxKind::NameExpr => {
                let s = self.sym_of(e);
                if let Some((l, _)) = self.find_local(s) {
                    return (!self.local_generics.contains_key(&l)).then(|| self.b.local_ty(l));
                }
                self.global_binding(s).map(|g| g.ty)
            }
            SyntaxKind::FieldExpr => {
                let base = self.peek_place_ty(*kids.first()?)?;
                let base = match self.ref_of(base) {
                    Some((inner, m)) => self.deref_ty(inner, m),
                    None => base,
                };
                let name = self.cx.src.text(self.cx.src.last(e));
                self.field_of(base, name).map(|(_, t)| t)
            }
            SyntaxKind::DerefExpr => {
                let (inner, m) = self.ref_of(self.peek_place_ty(*kids.first()?)?)?;
                Some(self.deref_ty(inner, m))
            }
            // A list element or a map value, of a collection or of a
            // reference to one; a slice is no place.
            SyntaxKind::IndexExpr => {
                let [base, key] = kids.as_slice() else {
                    return None;
                };
                if key.kind() == SyntaxKind::RangeExpr {
                    return None;
                }
                let base = self.peek_place_ty(*base)?;
                let base = match self.ref_of(base) {
                    Some((inner, m)) => self.deref_ty(inner, m),
                    None => base,
                };
                let base = self.infer.resolve(self.pool(), base);
                self.list_item(base).or_else(|| self.map_value_item(base))
            }
            _ => None,
        }
    }

    /// Whether the method `name` on a receiver of type `t` takes a
    /// reference receiver, and whether it is `&mut`.
    pub(crate) fn ref_receiver_named(&mut self, t: Ty, name: &str) -> StageResult<Option<bool>> {
        let t = self.norm_ty(t);
        if matches!(self.strip_mut(t), Ty::NEVER | Ty::POISON) {
            return Ok(None);
        }
        match self.resolve_method(t, name)? {
            Some(Hit::Inherent { method, .. }) => self.ref_receiver_of(method),
            // A trait value passes its payload to a reference receiver
            // itself (`trait.dyn.ref.member`), or a bare one has none
            // (`trait.dyn.ref.member`); the call decides.
            Some(Hit::Trait { choice, .. }) if choice.0 == ChoiceKind::TraitValue => Ok(None),
            Some(Hit::Trait { method, .. }) => self.ref_receiver_of(method),
            Some(Hit::Choice { method, .. }) => self.ref_receiver_of(method),
            Some(Hit::Pending { method, .. }) => self.ref_receiver_of(method),
            _ => Ok(None),
        }
    }

    /// The receiver of `base.name(...)` when the method takes `&self` or
    /// `&mut self` and `base` is place-shaped (`fn.method.auto-address`):
    /// the reference to the place, and the type of the value it refers
    /// to. A receiver that already is a reference is passed on. `None`
    /// when the call takes its receiver some other way.
    pub(crate) fn auto_address(
        &mut self,
        base: NodeRef<'_>,
        name: &str,
    ) -> StageResult<Option<(Ref, Ty)>> {
        let Some(bt) = self.peek_place_ty(base) else {
            return Ok(None);
        };
        let via_ref = self.ref_of(bt);
        let vt = match via_ref {
            Some((inner, m)) => self.deref_ty(inner, m),
            None => bt,
        };
        // A method promoted from an embedded struct takes the address of
        // that part of the place (`names.promoted.path`).
        let mut part = Vec::new();
        let mut mutable = self.ref_receiver_named(vt, name)?;
        if mutable.is_none()
            && self.is_struct_ty(vt)
            && let Some(path) = self.promoted_method_path(vt, name)
        {
            let mut pt = vt;
            for step in &path {
                let Some((idx, ft)) = self.field_of(pt, step) else {
                    return Ok(None);
                };
                part.push((idx, ft));
                pt = ft;
            }
            mutable = self.ref_receiver_named(pt, name)?;
        }
        let Some(mutable) = mutable else {
            return Ok(None);
        };
        if via_ref.is_some() && part.is_empty() {
            // A reference receiver: the method refers to what it refers to.
            let (r, t) = self.expr(base, None)?;
            return Ok(Some(
                self.pass_reference_receiver(r, t, name, base)?
                    .unwrap_or_else(|| self.poisoned(base)),
            ));
        }
        Ok(Some(self.address_of_place(base, name, mutable, part)?))
    }

    /// The reference that a call of `name`, a method taking `&self` or
    /// (`mutable`) `&mut self`, takes to the place-shaped receiver `base`
    /// that holds its value (`fn.method.auto-address`), with the type of
    /// that value. An error leaves a poisoned receiver.
    pub(crate) fn address_of_place(
        &mut self,
        base: NodeRef<'_>,
        name: &str,
        mutable: bool,
        part: Vec<(u32, Ty)>,
    ) -> StageResult<(Ref, Ty)> {
        let Some(op) = self.place_of(base, None, crate::refs::PlaceUse::Ref)? else {
            return Ok(self.poisoned(base));
        };
        let op = if part.is_empty() {
            op
        } else {
            self.part_place(op, part, base)
        };
        let place = match op {
            Operand::Place(p) => {
                if mutable && !self.receiver_writable(&p, base, name) {
                    // The receiver is still read, so its binding is used.
                    self.read_place(&p, base);
                    return Ok(self.poisoned(base));
                }
                if mutable && let Root::Local(l, _) = p.root {
                    self.ref_taken.push(l);
                }
                p
            }
            Operand::Value(v, t) => {
                if matches!(self.strip_mut(t), Ty::POISON | Ty::NEVER) || v.0 == NONE {
                    return Ok(self.poisoned(base));
                }
                if mutable {
                    self.not_a_place_receiver(name, base);
                    return Ok(self.poisoned(base));
                }
                self.new_object_place(v, t, base)
            }
        };
        // A receiver of a call: its `&mut` is held until the call returns,
        // and an element reference begins an access (`ref.elem.access`).
        let receiver = self
            .elem_ok
            .get(&crate::elem_refs::strip_parens(base).index())
            == Some(&crate::elem_refs::ElemUse::Receiver);
        if mutable {
            self.mut_place_taken(&place, base.index(), receiver);
        }
        if receiver && let Root::Elem(coll, ..) = place.root {
            self.begin_access(coll);
        }
        let pool = self.pool();
        let inner = self.strip_mut(place.ty);

        let rt = pool.intern_ty(&TyData::Ref { inner, mutable });
        let r = self.make_ref(&place, mutable, rt, base);
        Ok((r, place.ty))
    }

    /// The receiver of a store through an operator trait's `&mut self`
    /// method `name` (`IndexSet::index_set` for `r[k] = v`, `Update::update`
    /// for `v() = x`): a reference to the place `base` names, which must be
    /// writable (`fn.method.auto-address.place-writable`), or the `&mut`
    /// reference `base` evaluates to. Returns the reference and the type
    /// of the value it refers to, or `None` once an error is reported.
    pub(crate) fn store_receiver(
        &mut self,
        base: NodeRef<'_>,
        name: &str,
    ) -> StageResult<Option<(Ref, Ty)>> {
        if self
            .peek_place_ty(base)
            .is_some_and(|t| self.ref_of(t).is_none())
        {
            let (r, t) = self.address_of_place(base, name, true, Vec::new())?;
            return Ok((t != Ty::POISON).then_some((r, t)));
        }
        let (r, t) = self.expr(base, None)?;
        Ok(self.stored_through(r, t, name, base))
    }

    /// [`Self::store_receiver`] for a receiver that is no place: a `&mut`
    /// reference passes on; a read-only one, or a value, has nothing the
    /// store could change.
    pub(crate) fn stored_through(
        &mut self,
        r: Ref,
        t: Ty,
        name: &str,
        base: NodeRef<'_>,
    ) -> Option<(Ref, Ty)> {
        if r.0 == NONE || matches!(self.strip_mut(t), Ty::POISON | Ty::NEVER) {
            return None;
        }
        match self.ref_of(t) {
            Some((inner, true)) => {
                let pool = self.pool();
                let vt = self.deref_ty(inner, true);
                let rt = pool.intern_ty(&TyData::Ref {
                    inner: self.strip_mut(inner),
                    mutable: true,
                });
                Some((self.retype(r, rt, base.index()), vt))
            }
            Some((_, false)) => {
                self.read_only_receiver(name, t, base);
                None
            }
            None => {
                self.not_a_place_receiver(name, base);
                None
            }
        }
    }

    /// A receiver `r` of reference type `t` for a method `name` that takes
    /// a reference receiver: `r` itself, as the receiver's reference, with
    /// the type of the value it refers to. `None` when `t` is no reference
    /// or the method takes its receiver another way.
    pub(crate) fn pass_reference_receiver(
        &mut self,
        r: Ref,
        t: Ty,
        name: &str,
        base: NodeRef<'_>,
    ) -> StageResult<Option<(Ref, Ty)>> {
        let Some((inner, rm)) = self.ref_of(t) else {
            return Ok(None);
        };
        let vt = self.deref_ty(inner, rm);
        let Some(mutable) = self.ref_receiver_named(vt, name)? else {
            return Ok(None);
        };
        if mutable && !rm {
            self.read_only_receiver(name, t, base);
            return Ok(Some(self.poisoned(base)));
        }
        let pool = self.pool();
        let rt = pool.intern_ty(&TyData::Ref {
            inner: self.strip_mut(vt),
            mutable,
        });
        Ok(Some((self.retype(r, rt, base.index()), vt)))
    }

    /// The receiver `recv` of type `rt` for a method that takes a
    /// reference receiver (`mutable` for `&mut self`), when the call did not
    /// take the receiver's address already: a reference passes on, `&self`
    /// on another value refers to a new object holding it, and `&mut self`
    /// on a value that is no place is an error.
    pub(crate) fn fit_receiver(
        &mut self,
        recv: Ref,
        rt: Ty,
        mutable: bool,
        name: &str,
        n: NodeRef<'_>,
    ) -> Ref {
        if recv.0 == NONE || matches!(self.strip_mut(rt), Ty::POISON | Ty::NEVER) {
            return recv;
        }
        if self.ref_of(self.b.ty_of(recv)).is_some() {
            return recv;
        }
        // A list or a map that is no place, such as a map value, changes
        // in place as a nested write does (`types.coll.nested`): the call
        // refers to the collection where it is stored, with no copy.
        if mutable && self.is_coll_ty(rt) {
            if !self.coll_change(recv, rt, "this call", n) {
                return recv;
            }
            return self.new_object_ref(recv, rt, true, n).0;
        }
        if mutable {
            self.not_a_place_receiver(name, n);
            return recv;
        }
        let pool = self.pool();
        let inner = self.strip_mut(rt);
        let rty = pool.intern_ty(&TyData::Ref {
            inner,
            mutable: false,
        });
        let place = self.new_object_place(recv, rt, n);
        self.make_ref(&place, false, rty, n)
    }

    /// Calls may mutably address a temporary. Bound method references keep
    /// their separate writable-place requirement through `fit_receiver`.
    pub(crate) fn fit_call_receiver(
        &mut self,
        recv: Ref,
        rt: Ty,
        mutable: bool,
        name: &str,
        n: NodeRef<'_>,
    ) -> Ref {
        if recv.0 != NONE
            && !matches!(self.strip_mut(rt), Ty::POISON | Ty::NEVER)
            && mutable
            && self.ref_of(self.b.ty_of(recv)).is_none()
            && !self.is_coll_ty(rt)
        {
            return self.new_object_ref(recv, rt, true, n).0;
        }
        self.fit_receiver(recv, rt, mutable, name, n)
    }

    /// Whether `recv`, the receiver value of a method call, is a reference
    /// the call took (`auto_address`) for a receiver of type `rt`.
    pub(crate) fn addressed(&mut self, recv: Ref, rt: Ty) -> bool {
        recv.0 != NONE && self.ref_of(rt).is_none() && self.ref_of(self.b.ty_of(recv)).is_some()
    }

    fn poisoned(&mut self, n: NodeRef<'_>) -> (Ref, Ty) {
        (
            self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()),
            Ty::POISON,
        )
    }

    /// A new object holding `v`, as `&e` on a value makes one.
    fn new_object_place(&mut self, v: Ref, t: Ty, n: NodeRef<'_>) -> Place {
        let (l, d) = self.hold(v, t, n);
        self.add_flags(l, local_flags::CAPTURED_ASSIGNED);
        Place {
            root: Root::Local(l, d),
            path: vec![],
            ty: t,
        }
    }

    /// `&mut self` on a read-only reference `t`.
    fn read_only_receiver(&mut self, name: &str, t: Ty, base: NodeRef<'_>) {
        let msg = format!(
            "`{name}` takes `&mut self`, but the receiver is a read-only reference {}; take it with `&mut` to change what it refers to",
            self.show(t)
        );
        self.err(Code::NotWritable, base, &msg);
    }

    /// `fn.method.auto-address.place`: `&mut self` on a value that is no
    /// place.
    fn not_a_place_receiver(&mut self, name: &str, n: NodeRef<'_>) {
        let msg = format!(
            "`{name}` takes `&mut self`, which changes the receiver in place, but this receiver is not a variable or a field of one; bind it with `let mut` first"
        );
        self.err(Code::NotWritable, n, &msg);
    }

    /// Whether a `&mut self` method may take the address of the place `p`
    /// (`fn.method.auto-address.place-writable`); reports why not otherwise, with
    /// a fix-it that makes the binding writable.
    fn receiver_writable(&mut self, p: &Place, at: NodeRef<'_>, name: &str) -> bool {
        let ok = match p.root {
            Root::Local(l, _) => self.receiver_local_writable(l, at, name),
            Root::Global(s, _, _) => {
                let g = self.global_binding(s);
                if g.is_some_and(|g| g.mutable) {
                    true
                } else {
                    let binding = self.cx.names.text(s).to_owned();
                    let msg = format!(
                        "`{name}` takes `&mut self`, which changes `{binding}` in place, but `{binding}` is not `let mut`; bind it with `let mut`"
                    );
                    self.err(Code::NotWritable, at, &msg);
                    false
                }
            }
            Root::Deref(_, rt) => {
                if self.ref_of(rt).is_some_and(|(_, m)| m) {
                    true
                } else {
                    let msg = format!(
                        "`{name}` takes `&mut self`, but the receiver is reached through a read-only reference {}; take it with `&mut`",
                        self.show(rt)
                    );
                    self.err(Code::NotWritable, at, &msg);
                    false
                }
            }
            Root::Elem(list, _, lt) | Root::Slot(list, _, lt, _) => {
                self.coll_change(list, lt, "this call", at)
            }
            Root::DataField(_, _, _, dt) => {
                if self.has_mut_access(dt) {
                    true
                } else {
                    let msg = format!(
                        "`{name}` takes `&mut self`, but the receiver is a field of a readonly {}",
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
        // A list or a map value is no view (`ref.place.writable.collection`).
        if self.is_composite(p.ty) && !self.is_coll_value(p.ty) && !self.has_mut_access(p.ty) {
            let msg = format!(
                "`{name}` takes `&mut self`, but the receiver holds a readonly {}",
                self.show(p.ty)
            );
            self.err(Code::NotWritable, at, &msg);
            return false;
        }
        true
    }

    /// Whether the local `l` is writable for a `&mut self` receiver: a
    /// `let mut` binding or a `mut` parameter. Reports why not otherwise.
    fn receiver_local_writable(&mut self, l: LocalId, at: NodeRef<'_>, name: &str) -> bool {
        if self.mut_locals.contains(&l) {
            return true;
        }
        let (sym, flags, syn) = {
            let body = self.b.body_mut();
            (
                body.local_name[l.idx()],
                body.local_flags[l.idx()],
                body.local_syn[l.idx()],
            )
        };
        let binding = self.cx.names.text(sym).to_owned();
        let short = flags & local_flags::SHORT != 0;
        let using = self.using_locals.contains(&l);
        let param = flags & local_flags::PARAM != 0;
        let msg = if param && binding == "self" {
            format!(
                "`{name}` takes `&mut self`, but this method's `self` is its own read-only copy; take `&mut self` to change the caller's value"
            )
        } else if param {
            format!(
                "`{name}` takes `&mut self`, which changes `{binding}` in place, but parameter `{binding}` is the function's own copy, which never changes; write `mut {binding}`"
            )
        } else if short || using {
            format!(
                "`{name}` takes `&mut self`, which changes `{binding}` in place, but `{binding}` never changes; bind it with `let mut`"
            )
        } else {
            format!(
                "`{name}` takes `&mut self`, which changes `{binding}` in place, but `{binding}` is not `let mut`; bind it with `let mut`"
            )
        };
        self.err(Code::NotWritable, at, &msg);
        if !using {
            self.writable_binding_fix(&binding, short, param, syn);
        }
        false
    }

    /// The fix-it that makes the binding declared at `decl` writable:
    /// `let x` becomes `let mut x`, `x := e` becomes `let mut x = e`, a
    /// parameter `x: T` becomes `mut x: T`, and a receiver `self` becomes
    /// `&mut self`.
    pub(crate) fn writable_binding_fix(
        &mut self,
        binding: &str,
        short: bool,
        param: bool,
        decl: NodeIdx,
    ) {
        if decl == NodeIdx::NONE {
            return;
        }
        let src = self.cx.src;
        let mut node = src.parse.tree.node(decl);
        // A parameter's local is declared at its function.
        if param
            && node.kind() != SyntaxKind::Parameter
            && let Some(list) = hd_resolve::Src::child(node, SyntaxKind::ParameterList)
            && let Some(p) = list.children().find(|p| {
                p.kind() == SyntaxKind::Parameter
                    && if binding == "self" {
                        p.direct_token(&src.parse.tokens, TokenKind::KwSelfValue)
                            .is_some()
                    } else {
                        p.name(&src.parse.tokens)
                            .is_some_and(|t| src.text(t) == binding)
                    }
            })
        {
            node = p;
        }
        let file = src.file;
        let at = |lo: u32, hi: u32| hd_base::Span { file, lo, hi };
        match node.kind() {
            SyntaxKind::Parameter if param && binding == "self" => {
                let Some(tok) = node.direct_token(&src.parse.tokens, TokenKind::KwSelfValue) else {
                    return;
                };
                let span = src.token_span(tok);
                let edit = match node.direct_token(&src.parse.tokens, TokenKind::KwMut) {
                    Some(m) => (at(src.token_span(m).lo, span.hi), "&mut self"),
                    None => (at(span.lo, span.lo), "&mut "),
                };
                self.diags
                    .add_fix("take `&mut self`", FixSafety::Suggestion, &[edit]);
            }
            SyntaxKind::Parameter if param => {
                let Some(tok) = node.name(&src.parse.tokens) else {
                    return;
                };
                let lo = src.token_span(tok).lo;
                let title = format!("mark `{binding}` with `mut`");
                self.diags
                    .add_fix(&title, FixSafety::Suggestion, &[(at(lo, lo), "mut ")]);
            }
            SyntaxKind::BindingPattern => {
                let Some(tok) = src.tokens(node).find(|t| {
                    matches!(src.tkind(*t), Some(TokenKind::Ident | TokenKind::RawIdent))
                }) else {
                    return;
                };
                let lo = src.token_span(tok).lo;
                let title = format!("bind `{binding}` with `let mut`");
                if short {
                    // `x := e`: the token after the pattern is `:=`.
                    let next = hd_base::TokenIdx::from_raw(node.last_token().raw() + 1);
                    if src.tkind(next) != Some(TokenKind::ColonEq) {
                        return;
                    }
                    let op = src.token_span(next);
                    self.diags.add_fix(
                        &title,
                        FixSafety::Suggestion,
                        &[(at(lo, lo), "let mut "), (at(op.lo, op.hi), "=")],
                    );
                } else {
                    self.diags
                        .add_fix(&title, FixSafety::Suggestion, &[(at(lo, lo), "mut ")]);
                }
            }
            _ => {}
        }
    }
}
