//! Trait value references (spec 09 "Trait Value References"): `&dyn Tr`
//! and `&mut dyn Tr` are references whose target is a trait value. Only a
//! reference converts to one, as in Rust: `&S` and `&mut S` convert by
//! sharing what they refer to, so a `&mut self` method called through the
//! result changes the caller's place. A value is no reference, and the
//! program writes `&s`, `&mut s` or `&S { ... }` for it.
//!
//! A conversion is a `RefDyn`. With interior addresses, a trait value
//! reference is the trait value whose payload is the source reference
//! (`Coercion::RefToTraitValue`), as Rust's `&dyn Tr` is a pointer and a
//! vtable. On the Wasm GC emulation, its closure form, it is an ordinary
//! `std.core.Ref[dyn Tr]` (`crates/hd_wasm_linear/DESIGN.md` §15.2, "Trait value
//! references"): its `get` returns that trait value, and its identity
//! fields are the source reference's, so `is` compares the places.

use hd_base::{DefId, NodeIdx, TokenIdx};
use hd_diag::{Code, FixSafety};
use hd_resolve::ItemData;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{Callee, Coercion, NONE, Ref, Tag, TirSink, local_flags};
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
    /// value is `tv`: a reference to a concrete value shares it
    /// (`trait.dyn.ref.share`), and another trait value reference widens to
    /// a supertrait (`trait.dyn.ref.widen`). Any other value is a
    /// `type-mismatch` whose fix-it writes `&` or `&mut`
    /// (`trait.dyn.ref.written`). `None` when no rule applies, so the
    /// caller reports the mismatch.
    /// The mutability of a reference `got` already matches `mutable`.
    pub(crate) fn convert_to_dyn_ref(
        &mut self,
        r: Ref,
        got: Ty,
        (tv, mutable): (Ty, bool),
        n: NodeRef<'_>,
        what: &str,
    ) -> Option<Ref> {
        let pool = self.pool();
        let target = pool.intern_ty(&TyData::Ref { inner: tv, mutable });
        let g = self.infer.resolve(pool, got);
        if let Some((inner, _)) = self.ref_of(g) {
            let inner = self.strip_mut(self.infer.resolve(pool, inner));
            let inner = if let TyData::Infer(_) = pool.get(inner) {
                if let Some(default) = self
                    .infer
                    .kind_of(pool, inner)
                    .and_then(VarKind::literal_default)
                {
                    let _ = self.infer.unify(pool, inner, default);
                    default
                } else {
                    inner
                }
            } else {
                inner
            };
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
        // Any other value is no reference, and none is made for it
        // (`trait.dyn.ref.written`): the program writes `&` or `&mut`.
        let gs = self.strip_mut(g);
        let gs = match pool.get(gs) {
            TyData::Poison | TyData::Never => return None,
            TyData::Infer(_) => {
                let d = self
                    .infer
                    .kind_of(pool, gs)
                    .and_then(VarKind::literal_default)?;
                let _ = self.infer.unify(pool, gs, d);
                d
            }
            _ => gs,
        };
        // A mutable reference to a readonly view gets no fix-it.
        let mark = if mutable { "&mut" } else { "&" };
        let hint = match pool.get(gs) {
            _ if mutable && self.is_composite(g) && !self.has_mut_access(g) => Some(format!(
                "this readonly {} grants no writes, so pass `&mut` to a writable place",
                self.show(g)
            )),
            _ => None,
        };
        let msg = format!(
            "in {what}: expected {}, found {}; a value never becomes a trait value reference by itself, {}",
            self.show(target),
            self.show(g),
            hint.as_deref().map_or_else(
                || format!("so write `{mark}` before it"),
                |h| format!("and {h}")
            )
        );
        self.err(Code::TypeMismatch, n, &msg);
        if hint.is_none() {
            self.dyn_ref_value_fix(gs, tv, mutable, n);
        }
        Some(r)
    }

    /// The fix-it of `trait.dyn.ref.written.reference-fix.reference-values`: `&` or `&mut` before a
    /// place or a struct literal whose type `s`, without a `mut` view,
    /// implements the trait of `tv`.
    fn dyn_ref_value_fix(&mut self, s: Ty, tv: Ty, mutable: bool, n: NodeRef<'_>) {
        let value = match n.kind() {
            SyntaxKind::DataFieldInit | SyntaxKind::NamedArgument | SyntaxKind::Argument => {
                n.children().last()
            }
            _ => Some(n),
        };
        let Some(value) = value else {
            return;
        };
        if !matches!(
            value.kind(),
            SyntaxKind::NameExpr
                | SyntaxKind::FieldExpr
                | SyntaxKind::IndexExpr
                | SyntaxKind::PathExpr
                | SyntaxKind::DerefExpr
                | SyntaxKind::DataExpr
        ) {
            return;
        }
        if !self.dyn_fits(s, tv) {
            return;
        }
        let mark = if mutable { "&mut " } else { "&" };
        let span = self.cx.src.span(value);
        let at = hd_base::Span {
            hi: span.lo,
            ..span
        };
        self.diags.add_fix(
            &format!("write `{}`", mark.trim_end()),
            FixSafety::Exact,
            &[(at, mark)],
        );
    }

    /// Whether a reference to a value of type `s`, without a `mut` view,
    /// converts to a reference to the trait value `tv`: `s` implements its
    /// trait (`trait.dyn.ref.convert.impl`), or is a trait value of it or
    /// of a subtrait (`trait.dyn.ref.widen`).
    pub(crate) fn dyn_fits(&mut self, s: Ty, tv: Ty) -> bool {
        let pool = self.pool();
        let TyData::TraitValue { def, args, .. } = pool.get(tv) else {
            return false;
        };
        if let TyData::TraitValue {
            def: from,
            args: from_args,
            ..
        } = pool.get(s)
        {
            return from == def
                || supertrait_args(self.cx.lookup, pool, from, from_args, def, 0).is_some();
        }
        let tref = hd_types::solver::TraitRef {
            trait_: def,
            self_ty: s,
            args,
        };
        matches!(self.solve(tref), Ok(hd_types::solver::Answer::Holds { .. }))
    }

    /// The provider of the key `key`, whose trait is `def`, from the value
    /// `r` of type `t` (`req.with.type-or-ref`, "Reference Providers" in spec 11).
    /// Every provider keeps a trait value reference (`RefAsProvider`). A
    /// value provider for a trait without reference receivers is copied
    /// into a new object; an existing reference shares its place.
    pub(crate) fn install_provider(
        &mut self,
        r: Ref,
        t: Ty,
        (key, def): (Ty, DefId),
        n: NodeRef<'_>,
    ) -> Ref {
        let pool = self.pool();
        let mutable = self.trait_is_mutable(def, 0);
        if self.ref_of(t).is_none() && !self.has_ref_receiver_member(def) {
            let inner = self.strip_mut(self.infer.resolve(pool, t));
            self.require_dyn_impl(inner, key, n);
            let r = self.coerce(r, t, inner, n, "provider", ExpectPosition::General);
            let (source, source_ty) = self.shared_provider_object(r, inner, n);
            let r = self.shared_provider_ref(source, source_ty, key, mutable, n);
            return self
                .b
                .coerce(Coercion::RefAsProvider, NONE, r, key, n.index());
        }
        let want = pool.intern_ty(&TyData::Ref {
            inner: key,
            mutable,
        });
        let r = self.coerce(r, t, want, n, "provider", ExpectPosition::General);
        self.b
            .coerce(Coercion::RefAsProvider, NONE, r, key, n.index())
    }

    /// Reuse the runtime's generic object constructor. Local reference
    /// getter/setter closures would become sub-bodies of every provider site.
    fn shared_provider_object(&mut self, value: Ref, ty: Ty, n: NodeRef<'_>) -> (Ref, Ty) {
        let pool = self.pool();
        let ret = pool.intern_ty(&TyData::Ref {
            inner: ty,
            mutable: true,
        });
        let callee = Callee::Item {
            def: self.cx.names.item("std.rt", "host_provider"),
            targs: pool.list(&[ty]),
        };
        self.emit_call(&callee, &[value], ret, false, false, n)
    }

    /// A newly copied provider has the ordinary dyn-reference address form.
    /// Its GC form reuses shared getter/setter code and retains the concrete
    /// object's identity, so downcasting and `is` still name that same object.
    fn shared_provider_ref(
        &mut self,
        source: Ref,
        source_ty: Ty,
        key: Ty,
        mutable: bool,
        n: NodeRef<'_>,
    ) -> Ref {
        let pool = self.pool();
        let syn = n.index();
        let addr = self.b.open_block();
        let value = self.dyn_ref_value(source, source_ty, key, Coercion::RefToTraitValue, syn);
        let addr = self.b.close_block(addr, Some(value), key, syn);
        let form = self.b.open_block();
        let value = self.dyn_ref_value(source, source_ty, key, Coercion::RefToTraitValue, syn);
        let (wrapper, _) = self.shared_provider_object(value, key, n);
        let get_ty = pool.intern_ty(&TyData::Fn {
            params: pool.list(&[]),
            result: key,
            row: hd_types::RowId::EMPTY,
            suspends: false,
            inputs: hd_types::Inputs::Fixed,
        });
        let set_ty = pool.intern_ty(&TyData::Fn {
            params: pool.list(&[key]),
            result: Ty::VOID,
            row: hd_types::RowId::EMPTY,
            suspends: false,
            inputs: hd_types::Inputs::Fixed,
        });
        let get = self.b.emit(Tag::Field, wrapper.0, 0, get_ty, syn);
        let set = self.b.emit(Tag::Field, wrapper.0, 1, set_ty, syn);
        let any = self.any_ty();
        let base = self.b.emit(Tag::Field, source.0, 2, any, syn);
        let serial = self
            .b
            .emit(Tag::Field, source.0, 3, Ty::prim(Prim::I64), syn);
        let path = self.b.emit(Tag::Field, source.0, 4, Ty::STRING, syn);
        let rt = pool.intern_ty(&TyData::Ref {
            inner: key,
            mutable,
        });
        let rec = self.b.refs_record(&[get, set, base, serial, path]);
        let value = self.b.emit(Tag::NewData, NONE, rec, rt, syn);
        let form = self.b.close_block(form, Some(value), rt, syn);
        self.b.emit(Tag::RefDyn, addr.0, form.0, rt, syn)
    }

    /// Every requirement key yields a trait value reference; permission
    /// follows its receiver requirements.
    pub(crate) fn provider_ty(&self, key: Ty, def: DefId) -> (Ty, bool) {
        let mutable = self.trait_is_mutable(def, 0);
        (
            self.pool().intern_ty(&TyData::Ref {
                inner: key,
                mutable,
            }),
            true,
        )
    }

    /// `trait.dyn.ref.convert.impl`: the concrete type `s` implements the
    /// trait of `tv`.
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
        let _ = self.require_ref_as(tref, n, Code::TypeMismatch);
        if self.diags.len() != before {
            return;
        }
        self.check_value_bindings(s, tv, n);
    }

    /// The trait value reference for the source reference `src` of type
    /// `src_ty`: a `RefDyn` whose address form is the trait value that
    /// converts `src` with `kind` (`RefToTraitValue`, or `Supertrait` on
    /// the trait value `src` refers to), and whose closure form is a new
    /// `std.core.Ref[tv]` whose `get` makes that trait value, whose `set`
    /// is never called, and whose identity is `src`'s.
    pub(crate) fn build_dyn_ref(
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
        // As an address, the reference is the trait value itself, whose
        // payload is the source reference.
        let addr = self.b.open_block();
        let (s, _) = self.read_local(l, d, n);
        let v = self.dyn_ref_value(s, src_ty, tv, kind, syn);
        let addr = self.b.close_block(addr, Some(v), tv, syn);
        let form = self.b.open_block();
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
        let v = self.dyn_ref_value(s, src_ty, tv, kind, syn);
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
        let r = self.b.emit(Tag::NewData, NONE, rec, rt, syn);
        let form = self.b.close_block(form, Some(r), rt, syn);
        self.b.emit(Tag::RefDyn, addr.0, form.0, rt, syn)
    }

    /// The trait value `tv` that the trait value reference made from the
    /// source reference `s` of type `src_ty` refers to: `s` converted with
    /// `kind`.
    fn dyn_ref_value(&mut self, s: Ref, src_ty: Ty, tv: Ty, kind: Coercion, syn: NodeIdx) -> Ref {
        if kind == Coercion::Supertrait {
            let (dv, dt) = self.emit_deref(s, src_ty, syn);
            let dv = self.retype(dv, self.strip_mut(dt), syn);
            self.b.coerce(Coercion::Supertrait, NONE, dv, tv, syn)
        } else {
            self.b.coerce(Coercion::RefToTraitValue, NONE, s, tv, syn)
        }
    }

    /// Whether the trait or a supertrait requires an explicit reference
    /// provider through a `&self` or `&mut self` method.
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

    /// `trait.dyn.ref.member.mut`: a method that changes its receiver,
    /// `name`, called through a read-only `&dyn` reference whose receiver
    /// expression is at `recv`. The fix-it writes `&mut` in the binding's
    /// declared type when the receiver is a binding declared `&dyn ...`.
    pub(crate) fn dyn_ref_mut_receiver(&mut self, name: &str, recv: NodeIdx, n: NodeRef<'_>) {
        let msg = format!(
            "`{name}` changes its receiver, but this receiver is a read-only `&dyn` reference; take it as `&mut dyn` to call `{name}`"
        );
        self.err(Code::NotWritable, n, &msg);
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
