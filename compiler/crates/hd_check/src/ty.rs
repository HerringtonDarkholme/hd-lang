//! Type annotations inside bodies (type-checking.md §2.4): the same forms
//! as headers, read through the module scope, the item's type parameters
//! and the closure's interfaces. An `_` is a fresh variable.

use hd_base::{DefId, StageResult};
use hd_intern::PathKind;
use hd_resolve::{BindingKind, HeadKind, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{NONE, Ref, Tag, TirSink};
use hd_types::{ParamRef, Prim, RowData, RowId, Ty, TyData, TyList, VarKind};

use crate::body::{Ck, unsupported};

/// What a (possibly qualified) name in a body reaches.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Named {
    Item(DefId),
    Module(u32),
    /// Bound by a failed `use`: an earlier error, stay quiet.
    Poison,
}

impl Ck<'_, '_> {
    /// The identifier segments of a path node, in order.
    pub(crate) fn segments(&self, n: NodeRef<'_>) -> Vec<String> {
        n.direct_tokens()
            .filter(|t| {
                matches!(
                    self.cx.src.tkind(*t),
                    Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfType)
                )
            })
            .map(|t| self.cx.src.text(t).to_owned())
            .collect()
    }

    /// A module-level name: an item, a module, or nothing.
    pub(crate) fn scope_name(&self, name: &str) -> Option<Named> {
        let b = self.cx.scope.lookup(self.cx.names.syms.intern(name))?;
        match b.kind {
            BindingKind::Item => Some(Named::Item(DefId::from_raw(b.value))),
            BindingKind::Module => Some(Named::Module(b.value)),
            BindingKind::Poison => Some(Named::Poison),
            _ => None,
        }
    }

    /// Whether a module-level name was bound by a failed `use`.
    pub(crate) fn is_poison_name(&self, name: &str) -> bool {
        matches!(self.scope_name(name), Some(Named::Poison))
    }

    /// The value of a reference to a poisoned name.
    pub(crate) fn poison_value(&mut self, n: NodeRef<'_>) -> (Ref, Ty) {
        (
            self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()),
            Ty::POISON,
        )
    }

    /// `module.name` through the module's export.
    pub(crate) fn export(&self, module: u32, name: &str) -> Option<DefId> {
        let m = self.cx.scope.modules.get(module as usize)?;
        let sym = self.cx.names.syms.intern(name);
        self.cx
            .lookup
            .ifaces
            .iter()
            .find_map(|f| f.export(m, sym))
            .map(|e| e.def)
            .or_else(|| {
                let d = self.cx.names.item(m, name);
                self.cx.lookup.item(d).map(|_| d)
            })
    }

    /// A type parameter or `Self` in scope by name.
    pub(crate) fn type_param(&self, name: &str) -> Option<Ty> {
        if name == "Self" {
            return self.self_ty;
        }
        self.gens
            .iter()
            .rev()
            .find(|(s, _)| self.cx.names.text(*s) == name)
            .map(|(_, t)| *t)
    }

    pub(crate) fn kind_of_item(&self, d: DefId) -> Option<HeadKind> {
        self.cx.lookup.item(d).and_then(hd_resolve::Item::kind)
    }

    /// A type constructor applied to arguments (aliases expanded).
    pub(crate) fn ctor(&mut self, def: DefId, args: &[Ty]) -> StageResult<Ty> {
        let pool = self.pool();
        let Some(item) = self.cx.lookup.item(def) else {
            return unsupported(format!("the type {}", self.cx.names.path(def)));
        };
        let n = item.generics.len();
        let mut args = args.to_vec();
        while args.len() < n {
            let d = item.generics.get(args.len()).and_then(|g| g.default);
            args.push(match d {
                Some(t) => t,
                None => self.infer.fresh(pool, VarKind::General),
            });
        }
        Ok(match &item.data {
            ItemData::Alias(body) => pool.subst(*body, &|p: ParamRef| {
                (p.owner == def)
                    .then(|| args.get(p.index as usize).copied())
                    .flatten()
            }),
            ItemData::Data(_) | ItemData::Enum { .. } | ItemData::Newtype(_) => {
                pool.intern_ty(&TyData::Adt {
                    def,
                    args: pool.list(&args),
                })
            }
            _ => return unsupported("a non-type name in type position"),
        })
    }

    fn named_ty(&mut self, n: NodeRef<'_>) -> StageResult<Ty> {
        let pool = self.pool();
        let segs = self.segments(n);
        let mut args = Vec::new();
        let mut row = None;
        // A trait value's associated-type bindings (`dyn Supplier[Item = i32]`).
        let mut bound = Vec::new();
        if let Some(al) = Src::child(n, SyntaxKind::TypeArgumentList) {
            for a in al.children() {
                if a.kind() == SyntaxKind::RequirementRow {
                    row = Some(self.row_of(a)?);
                } else if a.kind() == SyntaxKind::AssociatedTypeBinding {
                    if let (Some(t), Some(ty)) =
                        (a.name(&self.cx.src.parse.tokens), Src::type_child(a))
                    {
                        let name = self.cx.src.text(t).to_owned();
                        bound.push((name, self.ty_node(ty)?));
                    }
                } else if a.kind().is_type() {
                    args.push(self.ty_node(a)?);
                }
            }
        }
        if segs.len() == 1 {
            let name = segs[0].as_str();
            if let Some(t) = self.type_param(name) {
                return Ok(t);
            }
            if name == "never" {
                return Ok(Ty::NEVER);
            }
            if let Some(p) = Prim::ALL.iter().find(|p| p.name() == name) {
                return Ok(Ty::prim(*p));
            }
        }
        if self.is_poison_name(&segs[0]) {
            return Ok(Ty::POISON);
        }
        let Some(def) = self.resolve_path(&segs) else {
            return unsupported(format!("the type name `{}`", segs.join(".")));
        };
        if self
            .cx
            .names
            .declared_in(def, self.cx.names.known.function_module)
        {
            let name = self
                .cx
                .names
                .paths
                .segment(hd_base::PathId::from_raw(def.raw()))
                .to_owned();
            if name == "Fn" || name == "SuspendFn" {
                let params = match args.first().map(|a| pool.get(*a)) {
                    Some(TyData::Tuple { elems, .. }) => elems,
                    Some(_) => pool.list(&args[..1]),
                    None => TyList::EMPTY,
                };
                return Ok(pool.intern_ty(&TyData::Fn {
                    params,
                    result: args.get(1).copied().unwrap_or(Ty::VOID),
                    row: row.unwrap_or(RowId::EMPTY),
                    suspends: name == "SuspendFn",
                }));
            }
        }
        if self.kind_of_item(def) == Some(HeadKind::Trait) {
            let bindings = bound
                .into_iter()
                .map(|(name, t)| (self.cx.names.member(def, PathKind::Member, &name), t))
                .collect();
            // A written trait value omits its defaulted trailing arguments
            // (`types.generic.default.written`); no `Self` is known here.
            let args = match self.cx.lookup.item(def) {
                Some(it) => hd_resolve::fill_trait_args(
                    self.cx.names.pool,
                    def,
                    &it.generics,
                    pool.list(&args),
                    None,
                ),
                None => pool.list(&args),
            };
            return Ok(pool.intern_ty(&TyData::TraitValue {
                def,
                args,
                bindings,
            }));
        }
        if def == self.cx.names.known.map
            && let Some(k) = args.first()
        {
            self.check_map_key(*k, n)?;
        }
        self.ctor(def, &args)
    }

    /// `Map[K < Eq & Hash, V]` (types.map-key.declared-bound): one error
    /// for a key type that misses either trait.
    pub(crate) fn check_map_key(&mut self, k: Ty, at: NodeRef<'_>) -> StageResult<()> {
        let pool = self.pool();
        let before = self.diags.len();
        for tr in [self.cx.names.known.eq, self.cx.names.known.hash] {
            let tref = hd_types::solver::TraitRef {
                trait_: tr,
                self_ty: k,
                args: hd_types::TyList::EMPTY,
            };
            if matches!(pool.get(self.infer.resolve(pool, k)), TyData::Param(_)) {
                return Ok(());
            }
            self.require_ref(tref, at)?;
            if self.diags.len() != before {
                break;
            }
        }
        Ok(())
    }

    /// A dotted path to an item.
    pub(crate) fn resolve_path(&self, segs: &[String]) -> Option<DefId> {
        let first = segs.first()?;
        match (self.scope_name(first)?, segs.len()) {
            (Named::Item(d), 1) => Some(d),
            (Named::Module(m), _) if segs.len() >= 2 => {
                if segs.len() == 2 {
                    return self.export(m, &segs[1]);
                }
                let base = self.cx.scope.modules.get(m as usize)?.clone();
                let module = format!("{base}.{}", segs[1..segs.len() - 1].join("."));
                let sym = self.cx.names.syms.intern(segs.last()?);
                self.cx
                    .lookup
                    .ifaces
                    .iter()
                    .find_map(|f| f.export(&module, sym))
                    .map(|e| e.def)
            }
            _ => None,
        }
    }

    /// A written row: keys, and the body's row parameters by name
    /// (`req.row.param.marked.use`).
    pub(crate) fn row_of(&mut self, n: NodeRef<'_>) -> StageResult<RowId> {
        let mut data = RowData::default();
        for c in n.children().filter(|c| c.kind() == SyntaxKind::NamedType) {
            let segs = self.segments(c);
            if let [name] = segs.as_slice()
                && let Some((_, p)) = self
                    .row_gens
                    .iter()
                    .rev()
                    .find(|(s, _)| self.cx.names.text(*s) == name.as_str())
            {
                data.params.push(*p);
                continue;
            }
            let t = self.named_ty(c)?;
            data.keys.push(t);
        }
        Ok(self.pool().row(&data))
    }

    /// The type a type node names.
    pub(crate) fn ty_node(&mut self, n: NodeRef<'_>) -> StageResult<Ty> {
        let pool = self.pool();
        let first = |me: &mut Self, n: NodeRef<'_>| -> StageResult<Ty> {
            match n.children().find(|c| c.kind().is_type()) {
                Some(c) => me.ty_node(c),
                None => Ok(Ty::VOID),
            }
        };
        Ok(match n.kind() {
            SyntaxKind::NamedType => self.named_ty(n)?,
            SyntaxKind::OptionalType => {
                let t = first(self, n)?;
                pool.intern_ty(&TyData::Option(t))
            }
            SyntaxKind::MutType => {
                let t = first(self, n)?;
                if matches!(pool.get(t), TyData::Prim(_)) {
                    self.err(
                        hd_diag::Code::MutOnPrimitive,
                        n,
                        "a primitive type has no `mut` form",
                    );
                    return Ok(t);
                }
                if matches!(pool.get(t), TyData::Tuple { .. }) {
                    self.err(
                        hd_diag::Code::MutOnTuple,
                        n,
                        "a tuple type has no `mut` form",
                    );
                    return Ok(t);
                }
                pool.intern_ty(&TyData::Mut(t))
            }
            SyntaxKind::ParenType => first(self, n)?,
            SyntaxKind::TupleType => {
                let mut elems = Vec::new();
                for c in n.children().filter(|c| c.kind().is_type()) {
                    elems.push(self.ty_node(c)?);
                }
                pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&elems),
                    rest: None,
                })
            }
            SyntaxKind::DynType => match Src::child(n, SyntaxKind::NamedType) {
                Some(c) => self.named_ty(c)?,
                None => return unsupported("this `dyn` form"),
            },
            SyntaxKind::FunctionType => {
                let arrow = n
                    .direct_token(&self.cx.src.parse.tokens, TokenKind::Arrow)
                    .map(hd_base::TokenIdx::raw);
                let suspends = n
                    .direct_token(&self.cx.src.parse.tokens, TokenKind::Bang)
                    .is_some();
                let mut params = Vec::new();
                let mut result = Ty::VOID;
                let mut row = RowId::EMPTY;
                for c in n.children() {
                    if c.kind() == SyntaxKind::RequirementRow {
                        row = self.row_of(c)?;
                    } else if c.kind().is_type() {
                        let after = arrow.is_some_and(|a| self.cx.src.first(c).raw() > a);
                        let t = self.ty_node(c)?;
                        if after {
                            result = t;
                        } else {
                            params.push(t);
                        }
                    }
                }
                pool.intern_ty(&TyData::Fn {
                    params: pool.list(&params),
                    result,
                    row,
                    suspends,
                })
            }
            SyntaxKind::InferType => self.infer.fresh(pool, VarKind::General),
            // A row in a row slot, as an explicit argument for a row
            // parameter (`req.row.slot.list`).
            SyntaxKind::RequirementRow => {
                let r = self.row_of(n)?;
                pool.intern_ty(&TyData::Row(r))
            }
            SyntaxKind::ProjectionType => {
                let base = first(self, n)?;
                let Some(t) = n.name(&self.cx.src.parse.tokens) else {
                    return unsupported("a projection without a name");
                };
                let name = self.cx.src.text(t).to_owned();
                let Some((trait_, args)) = self.assoc_owner(base, &name) else {
                    return unsupported("a projection on a type without a bound naming it");
                };
                let assoc = self.cx.names.member(trait_, PathKind::Member, &name);
                self.normalize(pool.intern_ty(&TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty: base,
                    args,
                }))?
            }
            other => return unsupported(format!("the type form {other:?} in a body")),
        })
    }
}
