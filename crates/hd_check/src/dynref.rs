//! Trait value references (spec 09 "Trait Value References"): `&dyn Tr`
//! and `&mut dyn Tr` are references whose target is a trait value. A
//! struct or other value converts to one by being copied into a new object
//! (Go copies a value stored in an interface); `&S` and `&mut S` convert by
//! sharing what they refer to, so a `&mut self` method called through the
//! result changes the caller's place.
//!
//! On the Wasm GC emulation a trait value reference is an ordinary
//! `std.core.Ref[dyn Tr]` (`crates/hd_wasm/DESIGN.md` §15.2, "Trait value
//! references"): its `get` returns a trait value whose payload is the
//! source reference (`Coercion::RefToTraitValue`), and its identity fields
//! are the source reference's, so `is` compares the places.

use hd_base::{DefId, NodeIdx, TokenIdx};
use hd_diag::{Code, FixSafety};
use hd_resolve::ItemData;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{Coercion, NONE, Ref, Tag, TirSink, local_flags};
use hd_types::{Prim, Ty, TyData, VarKind};

use crate::body::{Ck, ExpectPosition, OpenSub, supertrait_args};

impl Ck<'_, '_> {
    /// `(tv, mutable)` when `t` is a trait value reference `&dyn Tr` or
    /// `&mut dyn Tr`.
    pub(crate) fn dyn_ref_of(&self, t: Ty) -> Option<(Ty, bool)> {
        let pool = self.pool();
        let (inner, m) = self.ref_of(t)?;
        let inner = self.strip_mut(self.infer.resolve(pool, inner));
        matches!(pool.get(inner), TyData::TraitValue { .. }).then_some((inner, m))
    }

    /// Converts `r` of type `got` to the trait value reference whose trait
    /// value is `tv` (`trait.dyn.ref.convert`): a reference to a concrete
    /// value shares it, another trait value reference widens to a
    /// supertrait, and any other value is copied into a new object first.
    /// `None` when no rule applies, so the caller reports the mismatch.
    /// The mutability of a reference `got` already matches `mutable`.
    pub(crate) fn convert_to_dyn_ref(
        &mut self,
        r: Ref,
        got: Ty,
        (tv, mutable): (Ty, bool),
        n: NodeRef<'_>,
        what: &str,
        position: ExpectPosition,
    ) -> Option<Ref> {
        let pool = self.pool();
        let target = pool.intern_ty(&TyData::Ref { inner: tv, mutable });
        let g = self.infer.resolve(pool, got);
        if let Some((inner, _)) = self.ref_of(g) {
            let inner = self.strip_mut(self.infer.resolve(pool, inner));
            return match pool.get(inner) {
                TyData::Infer(_) | TyData::Poison | TyData::Never => None,
                TyData::TraitValue {
                    def: from,
                    args: from_args,
                    ..
                } => {
                    let TyData::TraitValue { def: to, .. } = pool.get(tv) else {
                        return None;
                    };
                    if from == to {
                        return None;
                    }
                    let widened = supertrait_args(self.cx.lookup, pool, from, from_args, to, 0)?;
                    let widened = pool.intern_ty(&TyData::TraitValue {
                        def: to,
                        args: widened,
                        bindings: vec![],
                    });
                    if !self.can_unify(widened, tv) {
                        return None;
                    }
                    Some(self.build_dyn_ref(r, g, tv, mutable, Coercion::Supertrait, n))
                }
                _ => {
                    self.require_dyn_impl(inner, tv, n);
                    Some(self.build_dyn_ref(r, g, tv, mutable, Coercion::RefToTraitValue, n))
                }
            };
        }
        // Any other value: a new object holds a copy (`trait.dyn.ref.copy`).
        let mut gs = self.strip_mut(g);
        match pool.get(gs) {
            TyData::Poison | TyData::Never => return None,
            TyData::Infer(_) => {
                let d = self
                    .infer
                    .kind_of(pool, gs)
                    .and_then(VarKind::literal_default)?;
                let _ = self.infer.unify(pool, gs, d);
                gs = d;
            }
            TyData::TraitValue { def, .. } if self.has_ref_receiver_member(def) => {
                self.bare_dyn_ref_error(def, n);
                return Some(r);
            }
            _ => {}
        }
        // `&mut` on a readonly view would grant writes through it
        // (`ref.expr.allocate.view`).
        if mutable && self.is_composite(g) && !self.has_mut_access(g) {
            let msg = format!(
                "in {what}: a readonly {} does not convert to {}, which could change it",
                self.show(g),
                self.show(target)
            );
            self.err(Code::MutableUpgrade, n, &msg);
            return Some(r);
        }
        let (l, d) = self.hold(r, g, n);
        self.add_flags(l, local_flags::CAPTURED_ASSIGNED);
        let place = crate::refs::Place {
            root: crate::refs::Root::Local(l, d),
            path: vec![],
            ty: g,
        };
        let rt = pool.intern_ty(&TyData::Ref { inner: gs, mutable });
        let made = self.make_ref(&place, gs, mutable, rt, n);
        Some(self.coerce(made, rt, target, n, what, position))
    }

    /// `trait.dyn.ref.convert.impl`: the concrete type `s` implements the
    /// trait of `tv`. A struct's `&mut self` method that fills a `mut self`
    /// trait method serves the reference (`trait.dyn.ref.bridge`).
    fn require_dyn_impl(&mut self, s: Ty, tv: Ty, n: NodeRef<'_>) {
        let pool = self.pool();
        let TyData::TraitValue { def, args, .. } = pool.get(tv) else {
            return;
        };
        let tref = hd_types::solver::TraitRef {
            trait_: def,
            self_ty: s,
            args,
        };
        let before = self.diags.len();
        let bridge = std::mem::replace(&mut self.bridge_ok, true);
        let ev = self.require_ref_as(tref, n, Code::TypeMismatch);
        self.bridge_ok = bridge;
        if self.diags.len() != before {
            return;
        }
        self.check_value_bindings(s, tv, n);
        // A default method the implementation does not write would pass a
        // bridged `&mut self` method a copy (`trait.dyn.ref.bridge.default`).
        if let Ok(Some(hd_types::solver::Evidence::Impl { row, .. })) = ev
            && let Some(imp) = self
                .cx
                .impl_table(row.module)
                .map(|t| t.def[row.row as usize])
            && let Some(ItemData::Trait(td)) = self.cx.lookup.item(def).map(|i| &i.data)
        {
            for &(name, tm) in &td.methods.clone() {
                let text = self.cx.names.text(name).to_owned();
                if self.bridge_default_call(tm, &[imp], &text, n) {
                    return;
                }
            }
        }
    }

    /// A new `std.core.Ref[tv]` for the source reference `src` of type
    /// `src_ty`: its `get` converts `src` with `kind`
    /// (`RefToTraitValue`, or `Supertrait` on the trait value `src` refers
    /// to), its `set` is never called, and its identity is `src`'s.
    fn build_dyn_ref(
        &mut self,
        src: Ref,
        src_ty: Ty,
        tv: Ty,
        mutable: bool,
        kind: Coercion,
        n: NodeRef<'_>,
    ) -> Ref {
        let pool = self.pool();
        let syn = n.index();
        let (l, d) = self.hold(src, src_ty, n);
        let get_ty = pool.intern_ty(&TyData::Fn {
            params: pool.list(&[]),
            result: tv,
            row: hd_types::RowId::EMPTY,
            suspends: false,
            inputs: hd_types::Inputs::Fixed,
        });
        let set_ty = pool.intern_ty(&TyData::Fn {
            params: pool.list(&[tv]),
            result: Ty::VOID,
            row: hd_types::RowId::EMPTY,
            suspends: false,
            inputs: hd_types::Inputs::Fixed,
        });
        let mark = self.b.open_sub(&[]);
        self.subs.push(OpenSub {
            mark,
            depth: self.scopes.len(),
        });
        let blk = self.b.open_block();
        let (s, _) = self.read_local(l, d, n);
        let v = if kind == Coercion::Supertrait {
            let (dv, dt) = self.emit_deref(s, src_ty, syn);
            let dv = self.retype(dv, self.strip_mut(dt), syn);
            self.b.coerce(Coercion::Supertrait, NONE, dv, tv, syn)
        } else {
            self.b.coerce(Coercion::RefToTraitValue, NONE, s, tv, syn)
        };
        let root = self.b.close_block(blk, Some(v), tv, syn);
        self.subs.pop();
        let get = self.b.close_sub(mark, root, get_ty, syn);
        // A trait value reference is never written whole (`*r = v` is
        // `trait.dyn.ref.no-store`), so its `set` is unreachable.
        let sym = self.cx.names.syms.intern("$value");
        let param = self.b.local(tv, sym, local_flags::PARAM, syn);
        let mark = self.b.open_sub(&[param]);
        self.subs.push(OpenSub {
            mark,
            depth: self.scopes.len(),
        });
        let blk = self.b.open_block();
        self.b.emit(Tag::Unreachable, NONE, NONE, Ty::NEVER, syn);
        let root = self.b.close_block(blk, None, Ty::VOID, syn);
        self.subs.pop();
        let set = self.b.close_sub(mark, root, set_ty, syn);
        let (s, _) = self.read_local(l, d, n);
        let any = self.any_ty();
        let base = self.b.emit(Tag::Field, s.0, 2, any, syn);
        let serial = self.b.emit(Tag::Field, s.0, 3, Ty::prim(Prim::I64), syn);
        let key = self.b.emit(Tag::Field, s.0, 4, Ty::STRING, syn);
        let rt = pool.intern_ty(&TyData::Ref { inner: tv, mutable });
        let rec = self.b.refs_record(&[get, set, base, serial, key]);
        self.b.emit(Tag::NewData, NONE, rec, rt, syn)
    }

    /// Whether the trait or one of its supertraits declares a `&self` or
    /// `&mut self` method, which a bare trait value cannot pass a
    /// reference to (`trait.dyn.member.ref-receiver-bare`).
    pub(crate) fn has_ref_receiver_member(&self, trait_: DefId) -> bool {
        let mut todo = vec![trait_];
        let mut seen = Vec::new();
        while let Some(t) = todo.pop() {
            if seen.contains(&t) || seen.len() > 64 {
                continue;
            }
            seen.push(t);
            let Some(ItemData::Trait(td)) = self.cx.lookup.item(t).map(|i| &i.data) else {
                continue;
            };
            let pool = self.pool();
            let by_ref = td.methods.iter().any(|&(_, m)| {
                self.cx
                    .lookup
                    .item(m)
                    .and_then(|i| i.sig())
                    .and_then(|s| s.params.first())
                    .is_some_and(|&(s, st)| {
                        self.cx.names.text(s) == "self"
                            && matches!(pool.get(st), TyData::Ref { .. })
                    })
            });
            if by_ref {
                return true;
            }
            for sup in &td.supers {
                if let TyData::TraitValue { def, .. } = pool.get(*sup) {
                    todo.push(def);
                }
            }
        }
        false
    }

    /// `trait.dyn.ref.bare`: a reference to a bare trait value of a trait
    /// with a `&self` or `&mut self` method.
    pub(crate) fn bare_dyn_ref_error(&mut self, trait_: DefId, n: NodeRef<'_>) {
        let name = self.cx.names.display_name(trait_);
        let msg = format!(
            "a bare `dyn {name}` holds no reference to pass to its `&self` and `&mut self` methods, so it does not become a `&dyn {name}`; convert the concrete value, or a reference to it, to `&dyn {name}` instead"
        );
        self.err(Code::DynMemberUnavailable, n, &msg);
    }

    /// `trait.dyn.ref.member.mut`: a method that changes its receiver,
    /// `name`, called through a read-only `&dyn` reference whose receiver
    /// expression is at `recv`. The fix-it writes `&mut` in the binding's
    /// declared type when the receiver is a binding declared `&dyn ...`.
    pub(crate) fn dyn_ref_mut_receiver(&mut self, name: &str, recv: NodeIdx, n: NodeRef<'_>) {
        let msg = format!(
            "`{name}` changes its receiver, but this receiver is a read-only `&dyn` reference; take it as `&mut dyn` to call `{name}`"
        );
        self.err(Code::MutableReceiverRequired, n, &msg);
        if recv == NodeIdx::NONE {
            return;
        }
        let src = self.cx.src;
        let base = src.parse.tree.node(recv);
        if base.kind() != SyntaxKind::NameExpr {
            return;
        }
        let Some((l, _)) = self.find_local(self.sym_of(base)) else {
            return;
        };
        let (sym, flags, decl) = {
            let body = self.b.body_mut();
            (
                body.local_name[l.idx()],
                body.local_flags[l.idx()],
                body.local_syn[l.idx()],
            )
        };
        if decl == NodeIdx::NONE {
            return;
        }
        let binding = self.cx.names.text(sym).to_owned();
        let mut node = src.parse.tree.node(decl);
        // A parameter's local is declared at its function.
        if flags & local_flags::PARAM != 0
            && node.kind() != SyntaxKind::Parameter
            && let Some(list) = hd_resolve::Src::child(node, SyntaxKind::ParameterList)
            && let Some(p) = list.children().find(|p| {
                p.kind() == SyntaxKind::Parameter
                    && p.name(&src.parse.tokens)
                        .is_some_and(|t| src.text(t) == binding)
            })
        {
            node = p;
        }
        let name_tok = match node.kind() {
            SyntaxKind::Parameter => node.name(&src.parse.tokens),
            SyntaxKind::BindingPattern => src
                .tokens(node)
                .find(|t| matches!(src.tkind(*t), Some(TokenKind::Ident | TokenKind::RawIdent))),
            _ => None,
        };
        let Some(tok) = name_tok else {
            return;
        };
        // `name: &dyn Tr`: the tokens after the name are `:`, `&`, then
        // the referenced type, which `mut` precedes.
        let at = |k: u32| TokenIdx::from_raw(tok.raw() + k);
        if src.tkind(at(1)) != Some(TokenKind::Colon)
            || src.tkind(at(2)) != Some(TokenKind::Amp)
            || src.tkind(at(3)) == Some(TokenKind::KwMut)
        {
            return;
        }
        let lo = src.token_span(at(3)).lo;
        let span = hd_base::Span {
            file: src.file,
            lo,
            hi: lo,
        };
        let title = format!("declare `{binding}` as `&mut dyn`");
        self.diags
            .add_fix(&title, FixSafety::Suggestion, &[(span, "mut ")]);
    }
}
