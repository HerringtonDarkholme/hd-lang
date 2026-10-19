//! Reference receivers (spec 07 "Methods And Receivers"): a method declared
//! `&self` takes `&Self`, one declared `&mut self` takes `&mut Self`, and a
//! call of one on a place takes the place's address, as Go does:
//! `p.translate(d)` means `(&mut p).translate(d)` (`fn.method.auto-address`).
//! `&mut self` needs a writable place (`fn.method.auto-address.writable`);
//! `&self` on a value that is no place refers to a new object holding it.
//!
//! The legacy bridge (`trait.impl.receiver-bridge`): until std's traits move
//! to `&mut self`, a struct's `&mut self` method fills a trait method
//! declared with `data`'s `mut self`. Calls of such a method on the struct
//! itself take its address as for any `&mut self` method; every use that
//! reaches the method through the trait's own declaration (a bound, a trait
//! value, a qualified call, a default body) would pass a copy instead, so
//! those are errors until the migration.
//!
//! On the Wasm GC emulation the address is a `std.core.Ref[T]`
//! (`crates/hd_wasm/DESIGN.md` §15.2, "References"), built by `refs.rs`.

use hd_base::{DefId, LocalId, NodeIdx, StageResult};
use hd_diag::{Code, FixSafety};
use hd_resolve::ItemData;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{ChoiceKind, NONE, Ref, Tag, TirSink, local_flags};
use hd_types::solver::Candidate;
use hd_types::{Ty, TyData};

use crate::body::Ck;
use crate::call::Hit;
use crate::refs::{Operand, Place, Root};

impl Ck<'_, '_> {
    /// Whether the method `method`, reached with the impl choice `choice`,
    /// takes a reference receiver, and whether it is `&mut`: its declared
    /// receiver is `&self` or `&mut self`, or it is a trait method declared
    /// `mut self` that the chosen impl fills with `&mut self` (the bridge).
    pub(crate) fn ref_receiver_of(
        &self,
        method: DefId,
        choice: Option<(ChoiceKind, u32)>,
    ) -> StageResult<Option<bool>> {
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
        if let Some((ChoiceKind::Impl, raw)) = choice
            && self.bridged_method(DefId::from_raw(raw), method).is_some()
        {
            return Ok(Some(true));
        }
        Ok(None)
    }

    /// [`Self::ref_receiver_of`] for a trait method whose impl is one of
    /// the candidates `cands`: the bridge applies when one of them fills
    /// it with `&mut self`.
    pub(crate) fn ref_receiver_cands(
        &self,
        method: DefId,
        cands: &[Candidate],
    ) -> StageResult<Option<bool>> {
        if let Some(m) = self.ref_receiver_of(method, None)? {
            return Ok(Some(m));
        }
        let bridged = cands.iter().any(|c| {
            self.cx.impl_table(c.row.module).is_some_and(|t| {
                self.bridged_method(t.def[c.row.row as usize], method)
                    .is_some()
            })
        });
        Ok(bridged.then_some(true))
    }

    /// The method of the impl `imp` that fills the trait method `tm`
    /// through the bridge (`trait.impl.receiver-bridge`): `tm` is declared
    /// `mut self` and the impl, of a struct, writes it with `&mut self`.
    pub(crate) fn bridged_method(&self, imp: DefId, tm: DefId) -> Option<DefId> {
        let pool = self.pool();
        let ItemData::Impl {
            methods, self_ty, ..
        } = &self.cx.lookup.item(imp)?.data
        else {
            return None;
        };
        if !self.is_struct_ty(*self_ty) {
            return None;
        }
        let name = self.cx.lookup.item(tm)?.name;
        let &(_, m) = methods.iter().find(|(n, _)| *n == name)?;
        let tsig = self.cx.lookup.item(tm)?.sig()?;
        let msig = self.cx.lookup.item(m)?.sig()?;
        let (Some(&(ts, tt)), Some(&(_, mt))) = (tsig.params.first(), msig.params.first()) else {
            return None;
        };
        let legacy = self.cx.names.text(ts) == "self" && matches!(pool.get(tt), TyData::Mut(_));
        let by_ref = matches!(pool.get(mt), TyData::Ref { mutable: true, .. });
        (legacy && by_ref).then_some(m)
    }

    /// `trait.impl.receiver-bridge.direct-or-ref`: an obligation `tref` that the
    /// evidence answers with an impl filling some method through the
    /// bridge is an error, since the trait's declaration would pass that
    /// method a copy. Reports at `at`; returns whether it did.
    pub(crate) fn bridge_only_direct(
        &mut self,
        tref: hd_types::solver::TraitRef,
        ev: &hd_types::solver::Evidence,
        at: NodeRef<'_>,
    ) -> bool {
        let hd_types::solver::Evidence::Impl { row, .. } = ev else {
            return false;
        };
        if self.bridge_ok {
            return false;
        }
        let Some(imp) = self
            .cx
            .impl_table(row.module)
            .map(|t| t.def[row.row as usize])
        else {
            return false;
        };
        let Some(m) = self.bridged_methods(imp).into_iter().next() else {
            return false;
        };
        let msg = format!(
            "{} implements {} with `&mut self` for `{}`, which the trait declares `mut self`; until the trait takes `&mut self`, that serves only calls on a {} itself, not this use through the trait",
            self.show(tref.self_ty),
            self.cx.names.display_name(tref.trait_),
            self.cx.names.text(m),
            self.show(self.strip_mut(tref.self_ty)),
        );
        self.err(Code::UnsatisfiedTraitBound, at, &msg);
        true
    }

    /// `trait.impl.receiver-bridge.other-uses`: a call on a struct of a default
    /// method of a trait whose impl, one of `impls`, fills some method
    /// through the bridge. The default body would pass that method a copy.
    /// Reports at `n`; returns whether it did.
    pub(crate) fn bridge_default_call(
        &mut self,
        method: DefId,
        impls: &[DefId],
        name: &str,
        n: NodeRef<'_>,
    ) -> bool {
        let Some(mname) = self.cx.lookup.item(method).map(|i| i.name) else {
            return false;
        };
        for &imp in impls {
            let Some(ItemData::Impl {
                methods, self_ty, ..
            }) = self.cx.lookup.item(imp).map(|i| &i.data)
            else {
                continue;
            };
            if methods.iter().any(|(n, _)| *n == mname) {
                continue;
            }
            let self_ty = *self_ty;
            let Some(b) = self.bridged_methods(imp).into_iter().next() else {
                continue;
            };
            let msg = format!(
                "`{name}` is a default method of the trait, whose body would pass `{}` a copy of the {}; write `{name}` in the implementation with `&mut self`",
                self.cx.names.text(b),
                self.show(self_ty)
            );
            self.err(Code::UnsatisfiedTraitBound, n, &msg);
            return true;
        }
        false
    }

    /// The names of the trait methods the impl `imp` fills through the
    /// bridge.
    fn bridged_methods(&self, imp: DefId) -> Vec<hd_base::Symbol> {
        let Some(ItemData::Impl {
            trait_, self_ty, ..
        }) = self.cx.lookup.item(imp).map(|i| &i.data)
        else {
            return Vec::new();
        };
        // Only a struct's implementation bridges.
        if !self.is_struct_ty(*self_ty) {
            return Vec::new();
        }
        let Some(ItemData::Trait(td)) = self.cx.lookup.item(*trait_).map(|t| &t.data) else {
            return Vec::new();
        };
        td.methods
            .iter()
            .filter(|&&(_, tm)| self.bridged_method(imp, tm).is_some())
            .map(|&(n, _)| n)
            .collect()
    }

    /// The type a place-shaped receiver expression would read, without
    /// evaluating it: a local or top-level binding, a field of one, or a
    /// value read through a reference (`*r`). `None` for any other
    /// expression.
    fn peek_place_ty(&self, e: NodeRef<'_>) -> Option<Ty> {
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
            // A list element, of a list or of a reference to one; a slice
            // is no place.
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
                self.list_item(self.infer.resolve(self.pool(), base))
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
            Some(Hit::Inherent { method, .. }) => self.ref_receiver_of(method, None),
            // A trait value passes its payload to a reference receiver
            // itself (`trait.dyn.ref.member`), or a bare one has none
            // (`trait.dyn.member.ref-receiver-bare`); the call decides.
            Some(Hit::Trait { choice, .. }) if choice.0 == ChoiceKind::TraitValue => Ok(None),
            Some(Hit::Trait { method, choice, .. }) => self.ref_receiver_of(method, Some(choice)),
            Some(Hit::Choice { method, cands, .. }) => self.ref_receiver_cands(method, &cands),
            Some(Hit::Pending { method, .. }) => self.ref_receiver_of(method, None),
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
        let Some(mutable) = self.ref_receiver_named(vt, name)? else {
            return Ok(None);
        };
        if via_ref.is_some() {
            // A reference receiver: the method refers to what it refers to.
            let (r, t) = self.expr(base, None)?;
            return Ok(Some(
                self.pass_reference_receiver(r, t, name, base)?
                    .unwrap_or_else(|| self.poisoned(base)),
            ));
        }
        // A list element pins its list for the call
        // (`fn.method.auto-address.element`).
        self.pin_for_call = true;
        let op = self.operand(base, None);
        self.pin_for_call = false;
        let Some(op) = op? else {
            return Ok(Some(self.poisoned(base)));
        };
        let place = match op {
            Operand::Place(p) => {
                if mutable && !self.receiver_writable(&p, base, name) {
                    // The receiver is still read, so its binding is used.
                    self.read_place(&p, base);
                    return Ok(Some(self.poisoned(base)));
                }
                if mutable && let Root::Local(l, _) = p.root {
                    self.ref_taken.push(l);
                }
                p
            }
            Operand::Value(v, t) => {
                if matches!(self.strip_mut(t), Ty::POISON | Ty::NEVER) || v.0 == NONE {
                    return Ok(Some(self.poisoned(base)));
                }
                if mutable {
                    self.not_a_place_receiver(name, base);
                    return Ok(Some(self.poisoned(base)));
                }
                self.new_object_place(v, t, base)
            }
        };
        let pool = self.pool();
        let inner = self.strip_mut(place.ty);
        let rt = pool.intern_ty(&TyData::Ref { inner, mutable });
        let r = self.make_ref(&place, inner, mutable, rt, base);
        Ok(Some((r, place.ty)))
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
            let msg = format!(
                "`{name}` takes `&mut self`, but the receiver is a read-only reference {}; take it with `&mut` to change what it refers to",
                self.show(t)
            );
            self.err(Code::MutableReceiverRequired, base, &msg);
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
        self.make_ref(&place, inner, false, rty, n)
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

    /// `fn.method.auto-address.place`: `&mut self` on a value that is no
    /// place.
    fn not_a_place_receiver(&mut self, name: &str, n: NodeRef<'_>) {
        let msg = format!(
            "`{name}` takes `&mut self`, which changes the receiver in place, but this receiver is not a variable or a field of one; bind it with `let mut` first"
        );
        self.err(Code::MutableReceiverRequired, n, &msg);
    }

    /// Whether a `&mut self` method may take the address of the place `p`
    /// (`fn.method.auto-address.writable`); reports why not otherwise, with
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
                    self.err(Code::MutableReceiverRequired, at, &msg);
                    false
                }
            }
            Root::Deref(_, _, rt) => {
                if self.ref_of(rt).is_some_and(|(_, m)| m) {
                    true
                } else {
                    let msg = format!(
                        "`{name}` takes `&mut self`, but the receiver is reached through a read-only reference {}; take it with `&mut`",
                        self.show(rt)
                    );
                    self.err(Code::MutableReceiverRequired, at, &msg);
                    false
                }
            }
            Root::Elem(list, _, _, _, _, lt) => self.coll_change(list, lt, "this call", at),
            Root::DataField(_, _, _, _, dt) => {
                if self.has_mut_access(dt) {
                    true
                } else {
                    let msg = format!(
                        "`{name}` takes `&mut self`, but the receiver is a field of a readonly {}",
                        self.show(dt)
                    );
                    self.err(Code::MutableReceiverRequired, at, &msg);
                    false
                }
            }
        };
        if !ok {
            return false;
        }
        if self.is_composite(p.ty) && !self.has_mut_access(p.ty) {
            let msg = format!(
                "`{name}` takes `&mut self`, but the receiver holds a readonly {}",
                self.show(p.ty)
            );
            self.err(Code::MutableReceiverRequired, at, &msg);
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
        self.err(Code::MutableReceiverRequired, at, &msg);
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
                let lo = src.token_span(tok).lo;
                self.diags.add_fix(
                    "take `&mut self`",
                    FixSafety::Suggestion,
                    &[(at(lo, lo), "&mut ")],
                );
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
