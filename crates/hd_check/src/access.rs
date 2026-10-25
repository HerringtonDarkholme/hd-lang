//! Access permission (`crates/hd_check/DESIGN.md` (type checking) §8; spec 04 "Composite Values And
//! Access Permission", "Mutable Paths"). Every expression's type carries
//! its access, `mut T` or `T`; unification ignores the marker, so the
//! permission rules are checked here, at bindings, coercions, field
//! reads and mutations.

use hd_diag::Code;
use hd_resolve::ItemData;
use hd_syntax::NodeRef;
use hd_tir::ir::{Coercion, Ref, Step, Tag};
use hd_types::{Ty, TyData, VarKind};

use crate::body::Ck;

/// A projection read ([`Ck::projection`]) and the value it reads from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Projection {
    /// A field of a struct, newtype or data value.
    Field(Ref),
    /// An element of a tuple.
    Element(Ref),
    /// A field of an enum value's payload.
    Payload(Ref),
}

impl Ck<'_, '_> {
    /// Whether a type is a composite reference type that has a readonly
    /// and a `mut` view: data, non-optional enums, collections and trait
    /// values.
    pub(crate) fn is_composite(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Mut(i) => self.is_composite(i),
            // A struct is a value with no views (`types.value.no-view`).
            // An enum is a value too: it changes by replacing the whole
            // value (`ref.place.writable.enum`).
            TyData::Adt { def, .. } => {
                !self.is_struct_def(def) && !self.is_enum_def(def) && !self.is_coll_def(def)
            }
            TyData::TraitValue { .. } => true,
            _ => false,
        }
    }

    /// Whether an expression of type `t` has mutable access
    /// (types.path.access): a `mut U` type. A type with no readonly view,
    /// or one not known yet, counts as mutable so that no error is reported
    /// on it.
    pub(crate) fn has_mut_access(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Mut(_) => true,
            TyData::Param(_) => false,
            // A struct and an enum are values with no readonly view: an
            // enum changes by replacing the whole value, so a writable
            // place holding one is writable (`ref.place.writable.enum`).
            TyData::Adt { def, .. } => {
                self.is_struct_def(def) || self.is_enum_def(def) || self.is_coll_def(def)
            }
            TyData::TraitValue { .. } => false,
            _ => true,
        }
    }

    /// Whether `def` is an `enum` declaration.
    pub(crate) fn is_enum_def(&self, def: hd_base::DefId) -> bool {
        self.cx
            .lookup
            .item(def)
            .is_some_and(|i| matches!(i.data, hd_resolve::ItemData::Enum { .. }))
    }

    /// Whether `def` is `List` or `Map`, a copy-on-write value
    /// (`types.coll.value`).
    pub(crate) fn is_coll_def(&self, def: hd_base::DefId) -> bool {
        let known = self.cx.names.known;
        def == known.list || def == known.map
    }

    /// Whether `def` is a `struct` declaration (`data.struct.decl`).
    pub(crate) fn is_struct_def(&self, def: hd_base::DefId) -> bool {
        self.cx.lookup.item(def).is_some_and(|i| i.is_struct)
    }

    /// Whether `t` is a struct type, `mut` or not (`types.value.struct`).
    pub(crate) fn is_struct_ty(&self, t: Ty) -> bool {
        let pool = self.pool();
        match pool.get(self.strip_mut(t)) {
            TyData::Adt { def, .. } => self.is_struct_def(def),
            _ => false,
        }
    }

    /// Whether `t` is a data type, `mut` or not: a value of it is the
    /// address of an object whose payload holds its fields, so a field
    /// read dereferences it (`Read [*%o .k]`).
    pub(crate) fn is_data_object_ty(&self, t: Ty) -> bool {
        let pool = self.pool();
        match pool.get(self.strip_mut(t)) {
            TyData::Adt { def, .. } => {
                self.cx.lookup.item(def).is_some_and(|i| {
                    !i.is_struct && matches!(i.data, hd_resolve::ItemData::Data(_))
                })
            }
            _ => false,
        }
    }

    /// Field `k`, of type `ft`, of the value `base`: `Read [%base .k]` of
    /// a struct, newtype or tuple value, `Read [*%base .k]` of a data
    /// value, whose fields live in its object.
    pub(crate) fn field_value(&mut self, base: Ref, k: u32, ft: Ty, at: hd_base::NodeIdx) -> Ref {
        if self.is_data_object_ty(self.b.ty_of(base)) {
            self.b.data_field(base, k, ft, at)
        } else {
            self.b.field(base, k, ft, at)
        }
    }

    /// What the projection read `r` reads, and from which value: a field
    /// of a struct, newtype or data value (`[%v .k]`, `[*%o .k]`), an
    /// element of a tuple (`[%v .k]`), or a payload field
    /// (`[%v as .V .k]`). `None` for any other value, a read through a
    /// reference among them.
    pub(crate) fn projection(&mut self, r: Ref) -> Option<Projection> {
        let i = r.as_inst()?.0 as usize;
        let (base, step) = self.b.body_mut().read_step(i)?;
        let base = Ref(base);
        let bt = self.b.ty_of(base);
        Some(match step {
            Step::Field(_)
                if matches!(self.pool().get(self.strip_mut(bt)), TyData::Tuple { .. }) =>
            {
                Projection::Element(base)
            }
            Step::Field(_) => Projection::Field(base),
            Step::DerefField(_) if self.is_data_object_ty(bt) => Projection::Field(base),
            Step::DerefField(_) => return None,
            Step::Variant(..) => Projection::Payload(base),
        })
    }

    /// The list and the index an element read `r` reads,
    /// `Read [%xs [%i]]` ([`hd_tir::ir::Body::element_read`]).
    pub(crate) fn element_of(&mut self, r: Ref) -> Option<(Ref, Ref)> {
        let i = r.as_inst()?.0 as usize;
        let (list, index) = self.b.body_mut().element_read(i)?;
        Some((Ref(list), Ref(index)))
    }

    /// Whether `let mut` may bind a value of type `t`: mutable access, or
    /// an optional of a `mut Suspend[T]` payload or of a type that is not
    /// composite.
    pub(crate) fn let_mut_ok(&self, t: Ty) -> bool {
        let pool = self.pool();
        let r = self.infer.resolve(pool, t);
        match pool.get(r) {
            TyData::Option(i) => {
                let i = self.infer.resolve(pool, i);
                matches!(pool.get(i), TyData::Mut(_)) || !self.is_composite(i)
            }
            _ => self.has_mut_access(r),
        }
    }

    /// The readonly form of `t`: its outer `mut` removed, which only a
    /// `mut Suspend[T]` has (types.mut.weaken).
    pub(crate) fn readonly_view(&self, t: Ty) -> Ty {
        let pool = self.pool();
        let r = self.infer.resolve(pool, t);
        match pool.get(r) {
            TyData::Mut(i) => i,
            _ => t,
        }
    }

    /// The type of the field read `e.name` whose substituted type is `ft`:
    /// a field declared `mut Suspend[T]` loses its `mut` through an `e`
    /// that is not writable.
    pub(crate) fn field_access(&self, base: Ty, name: &str, ft: Ty) -> Ty {
        let pool = self.pool();
        let TyData::Adt { def, .. } = pool.get(self.strip_mut(base)) else {
            return ft;
        };
        let Some(ItemData::Data(fields)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return ft;
        };
        let sym = self.cx.names.syms.intern(name);
        let mut_field = fields
            .iter()
            .any(|f| f.name == sym && matches!(pool.get(f.ty), TyData::Mut(_)));
        if mut_field && !self.has_mut_access(base) {
            self.readonly_view(ft)
        } else {
            ft
        }
    }

    /// A store `e.f = v` or `e[i] = v` needs mutable access to `e`
    /// (types.path.store.not-writable).
    pub(crate) fn check_store_target(&mut self, (br, bt): (Ref, Ty), at: NodeRef<'_>) {
        // An element store changes a collection value through a writable
        // place (`types.coll.writable`).
        if self.is_coll_ty(bt) {
            self.coll_change(br, bt, "this store", at);
            return;
        }
        if self.has_mut_access(bt) {
            return;
        }
        let msg = format!(
            "{} is a readonly view; mutation needs `mut` access",
            self.show(bt)
        );
        self.err(Code::NotWritable, at, &msg);
    }

    /// A `mut self` call needs mutable access to its receiver
    /// (types.path.store.not-writable). `self_param` is the method's
    /// declared receiver type.
    pub(crate) fn check_receiver(&mut self, self_param: Ty, recv: Ref, rt: Ty, at: NodeRef<'_>) {
        let pool = self.pool();
        let TyData::Mut(inner) = pool.get(self_param) else {
            return;
        };
        // A primitive `Self` has no `mut` form (types.prim.no-mut.self-call).
        if matches!(pool.get(self.strip_mut(inner)), TyData::Prim(_)) {
            return;
        }
        if self.has_mut_access(rt) {
            return;
        }
        let binding = recv.as_inst().and_then(|i| {
            let body = self.b.body_mut();
            let local = body.local_read(i.0 as usize)?;
            if body.local_flags[local.idx()] & hd_tir::ir::local_flags::PARAM != 0 {
                return None;
            }
            let value = (0..body.len()).find_map(|j| {
                (body.stored_local(j) == Some((local, true))).then_some(Ref(body.data[j][1]))
            })?;
            let weakened = value.as_inst()?.0 as usize;
            if body.tags[weakened] != Tag::Coerce
                || body.record(body.data[weakened][1]).first() != Some(&(Coercion::Weaken as u32))
            {
                return None;
            }
            let value = Ref(body.data[weakened][0]);
            Some((body.local_name[local.idx()], value))
        });
        let shown = self.show(rt);
        let hint = match binding {
            Some((name, value))
                if matches!(
                    pool.get(self.infer.resolve(pool, self.b.ty_of(value))),
                    TyData::Mut(_)
                ) =>
            {
                let name = self.cx.names.text(name);
                format!(
                    "; bind with `let mut {name} = ...` to keep the initializer's mutable access"
                )
            }
            _ => String::new(),
        };
        let msg =
            format!("this method takes `mut self`, but the receiver is a readonly {shown}{hint}");
        self.err(Code::NotWritable, at, &msg);
    }

    /// A readonly value where `mut` is wanted (types.mut.no-upgrade,
    /// types.mut.argument). Returns whether it reported.
    pub(crate) fn check_upgrade(&mut self, got: Ty, want: Ty, n: NodeRef<'_>, what: &str) -> bool {
        let pool = self.pool();
        let w = self.infer.shallow(pool, want);
        let TyData::Mut(wi) = pool.get(w) else {
            return false;
        };
        if !self.is_composite(wi) || !self.is_composite(got) || self.has_mut_access(got) {
            return false;
        }
        // A declared `mut` parameter, not one generic inference made `mut`.
        // Building a `mut` trait value from a readonly value is an upgrade
        // by the conversion (types.mut.no-upgrade).
        let erases = matches!(
            pool.get(self.infer.resolve(pool, wi)),
            TyData::TraitValue { .. }
        ) && !matches!(pool.get(self.strip_mut(got)), TyData::TraitValue { .. });
        let declared = matches!(pool.get(want), TyData::Mut(_)) && !erases;
        if what == "argument" && declared {
            let msg = format!(
                "the parameter takes {}, the argument is a readonly {}",
                self.show(want),
                self.show(got)
            );
            self.err(Code::ReadonlyArgumentToMutableParameter, n, &msg);
        } else {
            let msg = format!(
                "in {what}: a readonly {} cannot become {}",
                self.show(got),
                self.show(want)
            );
            self.err(Code::MutableUpgrade, n, &msg);
        }
        true
    }

    /// Whether a trait, or one of its supertraits, has a `mut self` or
    /// `&mut self` method: a mutable requirement trait
    /// (`req.mut.trait-changing`).
    pub(crate) fn trait_is_mutable(&self, tr: hd_base::DefId, depth: u32) -> bool {
        let pool = self.pool();
        let Some(ItemData::Trait(t)) = self.cx.lookup.item(tr).map(|i| &i.data) else {
            return false;
        };
        if depth > 16 {
            return false;
        }
        t.methods.iter().any(|(_, m)| {
            self.cx
                .lookup
                .item(*m)
                .and_then(|i| i.sig())
                .and_then(|s| s.params.first())
                .is_some_and(|(s, st)| {
                    self.cx.names.text(*s) == "self"
                        && matches!(
                            pool.get(*st),
                            TyData::Mut(_) | TyData::Ref { mutable: true, .. }
                        )
                })
        }) || t.supers.iter().any(|s| match pool.get(*s) {
            TyData::TraitValue { def, .. } => self.trait_is_mutable(def, depth + 1),
            _ => false,
        })
    }

    /// Whether `got` converts to `want` by permission alone, once the two
    /// unify: weakening `mut U` to `U` where a readonly outer view's
    /// declared variance allows it, exact permissions elsewhere
    /// (types.variance.readonly-only, types.variance.mut-invariant,
    /// types.option.invariant). `weaken` is false for an invariant place.
    pub(crate) fn perm_fits(&self, got: Ty, want: Ty, weaken: bool, depth: u32) -> bool {
        let pool = self.pool();
        let g = self.infer.resolve(pool, got);
        let w = self.infer.resolve(pool, want);
        if depth > 32 || g == w {
            return true;
        }
        let (gm, gi) = match pool.get(g) {
            TyData::Mut(i) => (true, i),
            _ => (false, g),
        };
        let (wm, wi) = match pool.get(w) {
            TyData::Mut(i) => (true, i),
            _ => (false, w),
        };
        if gm != wm && self.is_composite(gi) && self.is_composite(wi) && (wm || !weaken) {
            return false;
        }
        let d = depth + 1;
        match (pool.get(gi), pool.get(wi)) {
            (TyData::Adt { def, args: ga }, TyData::Adt { def: d2, args: wa }) if def == d2 => {
                let vs = self.declared_variances(def);
                pool.list_items(ga)
                    .iter()
                    .copied()
                    .zip(pool.list_items(wa).iter().copied())
                    .enumerate()
                    .all(|(j, (a, b))| {
                        // A `mut` outer view is invariant in every argument.
                        let v = if wm {
                            0
                        } else {
                            vs.get(j).copied().unwrap_or(0)
                        };
                        match v {
                            1 => self.perm_fits(a, b, true, d),
                            -1 => self.perm_fits(b, a, true, d),
                            _ => self.perm_fits(a, b, false, d),
                        }
                    })
            }
            (TyData::Option(a), TyData::Option(b)) => self.perm_fits(a, b, false, d),
            // `ref.type.distinct`: a reference type is invariant in its
            // operand, whose own outer `mut` is no part of it.
            (
                TyData::Ref {
                    inner: a,
                    mutable: am,
                    ..
                },
                TyData::Ref {
                    inner: b,
                    mutable: bm,
                    ..
                },
            ) => {
                (am == bm || (weaken && am && !bm))
                    && self.perm_fits(self.strip_mut(a), self.strip_mut(b), false, d)
            }
            (TyData::Tuple { elems: a, .. }, TyData::Tuple { elems: b, .. }) => pool
                .list_items(a)
                .iter()
                .copied()
                .zip(pool.list_items(b).iter().copied())
                .all(|(x, y)| self.perm_fits(x, y, weaken, d)),
            (TyData::TraitValue { args: a, .. }, TyData::TraitValue { args: b, .. }) => pool
                .list_items(a)
                .iter()
                .copied()
                .zip(pool.list_items(b).iter().copied())
                .all(|(x, y)| self.perm_fits(x, y, false, d)),
            // `fn.type.declared-variance`: contravariant in each input,
            // covariant in the output.
            (
                TyData::Fn {
                    params: a,
                    result: ar,
                    row: aw,
                    ..
                },
                TyData::Fn {
                    params: b,
                    result: br,
                    row: bw,
                    ..
                },
            ) => {
                // `types.variance.function`: the row is invariant, so only
                // a function value at the top widens it
                // (`req.row.subsume`); a function type inside another type
                // keeps its row exactly.
                (depth == 0 || self.rows_equal(aw, bw))
                    && pool
                        .list_items(a)
                        .iter()
                        .copied()
                        .zip(pool.list_items(b).iter().copied())
                        .all(|(x, y)| self.perm_fits(y, x, true, d))
                    && self.perm_fits(ar, br, true, d)
            }
            _ => true,
        }
    }

    /// The permission join: a variable an earlier argument solved as
    /// `mut X` is lowered to `X` when this argument is a readonly `X`
    /// (types.generic.infer.join).
    pub(crate) fn join_down(&mut self, var: Ty, got: Ty) -> bool {
        let pool = self.pool();
        let TyData::Mut(x) = pool.get(self.infer.shallow(pool, var)) else {
            return false;
        };
        if self.is_composite(got) && !self.has_mut_access(got) && self.can_unify(got, x) {
            self.infer.rebind(pool, var, x);
            return true;
        }
        false
    }

    /// Whether `t` is a composite type, or an optional of one.
    fn composite_or_optional(&self, t: Ty) -> bool {
        let pool = self.pool();
        let r = self.infer.resolve(pool, t);
        match pool.get(r) {
            TyData::Option(i) => self.is_composite(i),
            _ => self.is_composite(r),
        }
    }

    /// A plain `let mut name` binding of type `t`, local or top-level. A local of a primitive type is valid,
    /// since `&mut` of it needs `let mut`, and so is a local whose
    /// `mut Suspend[T]` annotation already grants `mut`: the caller judges
    /// either at the end of the body, by whether `&mut` of it is taken, and
    /// this returns it (types.bind.let-mut-primitive-local). A
    /// top-level primitive one stays an error
    /// (types.bind.let-mut-primitive-top).
    pub(crate) fn let_mut_binding(
        &mut self,
        t: Ty,
        annotated: bool,
        local: bool,
        at: NodeRef<'_>,
    ) -> Option<crate::body::LetMutKind> {
        let pool = self.pool();
        let lit = matches!(
            self.infer.kind_of(pool, t),
            Some(VarKind::IntLit | VarKind::SignedIntLit | VarKind::FloatLit)
        );
        match pool.get(self.strip_mut(t)) {
            _ if lit && local => return Some(crate::body::LetMutKind::Primitive),
            TyData::Prim(_) if local => return Some(crate::body::LetMutKind::Primitive),
            _ if local && annotated && self.composite_or_optional(t) && self.let_mut_ok(t) => {
                return Some(crate::body::LetMutKind::Annotated);
            }
            _ if lit => self.err(
                Code::MutOnPrimitive,
                at,
                "`let mut` on a primitive top-level binding; a plain `let` is already reassignable",
            ),
            TyData::Prim(_) => {
                self.err(
                    Code::MutOnPrimitive,
                    at,
                    "`let mut` on a primitive top-level binding; a plain `let` is already reassignable",
                );
            }
            _ => {}
        }
        None
    }
}
