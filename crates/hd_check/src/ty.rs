//! Type annotations inside bodies (`crates/hd_check/DESIGN.md` (type checking) §2.4): the same forms
//! as headers, read through the module scope, the item's type parameters
//! and the closure's interfaces. An `_` is a fresh variable.

use hd_base::{DefId, Span, StageResult};
use hd_diag::Code;
use hd_intern::PathKind;
use hd_resolve::{BindingKind, HeadKind, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{NONE, Ref, Tag, TirSink};
use hd_types::{Inputs, ParamRef, Prim, RowData, RowId, Ty, TyData, VarKind};

use crate::body::{Ck, unsupported};

/// What a (possibly qualified) name in a body reaches.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Named {
    Item(DefId),
    Module(u32),
    /// Bound by a failed `use`: an earlier error, stay quiet.
    Poison,
    /// A module path to a declaration its module does not export: reported
    /// when the path was resolved.
    Missing,
}

/// What a written name must be: a type, or a trait (a requirement key,
/// a `dyn` type). It decides the code of a name that reaches nothing,
/// and a trait role rejects a name that reaches a type.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameRole {
    Type,
    Trait,
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

    /// An item name: a local declaration in scope here, else a
    /// module-level name (an item, a module, or nothing).
    pub(crate) fn scope_name(&self, name: &str) -> Option<Named> {
        let sym = self.cx.names.syms.intern(name);
        if let Some(l) = hd_resolve::local_at(self.cx.locals, sym, self.at) {
            return Some(Named::Item(l.def));
        }
        let Some(b) = self.cx.scope_at(self.at).lookup(sym) else {
            // A prelude name of test code (`module.prelude.test-only`).
            if self.in_tests_block() {
                return hd_resolve::test_prelude_item(&self.cx.names, name).map(Named::Item);
            }
            return None;
        };
        match b.kind {
            BindingKind::Item => Some(Named::Item(DefId::from_raw(b.value))),
            BindingKind::Module => Some(Named::Module(b.value)),
            BindingKind::Poison => Some(Named::Poison),
            _ => None,
        }
    }

    /// `module.Item` as the base of a member (`cmp.Ordering.Equal`,
    /// `text.StringBuilder::new`): the module's declaration, `Missing` once
    /// the unknown or private member is reported, or `None` when `n` is not
    /// a name through a module.
    pub(crate) fn module_item(&mut self, n: NodeRef<'_>) -> Option<Named> {
        if n.kind() != SyntaxKind::FieldExpr {
            return None;
        }
        let base = n.children().next()?;
        if base.kind() != SyntaxKind::NameExpr || self.find_local(self.sym_of(base)).is_some() {
            return None;
        }
        let text = self.cx.src.text(self.cx.src.first(base)).to_owned();
        let Some(Named::Module(m)) = self.scope_name(&text) else {
            return None;
        };
        let name = self.cx.src.text(self.cx.src.last(n)).to_owned();
        let Some(def) = self.export(m, &name) else {
            self.missing_export(m, &name, n);
            return Some(Named::Missing);
        };
        Some(Named::Item(def))
    }

    /// Whether a module-level name was bound by a failed `use`.
    pub(crate) fn is_poison_name(&self, name: &str) -> bool {
        matches!(self.scope_name(name), Some(Named::Poison))
    }

    /// The value of a reference to a poisoned name.
    pub(crate) fn poison_value(&mut self, n: NodeRef<'_>) -> (Ref, Ty) {
        self.read_poison_name = true;
        (
            self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()),
            Ty::POISON,
        )
    }

    /// `module.name` through the module's export.
    pub(crate) fn export(&self, module: u32, name: &str) -> Option<DefId> {
        let m = self.cx.scope_at(self.at).modules.get(module as usize)?;
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

    /// A module path `module.name` whose module exports no `name`:
    /// `private-import` when the module declares it without `pub`
    /// (`expr.name.qualified.private`), else `unknown-import`
    /// (`expr.name.qualified.missing`).
    pub(crate) fn missing_export(&mut self, module: u32, name: &str, n: NodeRef<'_>) {
        let path = self
            .cx
            .scope_at(self.at)
            .modules
            .get(module as usize)
            .cloned()
            .unwrap_or_default();
        let sym = self.cx.names.syms.intern(name);
        if self
            .cx
            .lookup
            .ifaces
            .iter()
            .any(|f| f.is_private(&path, sym))
        {
            let msg = format!("`{path}` declares `{name}` without `pub`");
            self.err(Code::PrivateImport, n, &msg);
        } else {
            let msg = format!("`{path}` declares no `{name}`");
            self.err(Code::UnknownImport, n, &msg);
        }
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

    /// Whether `t` may be the inputs argument of `Fn` or `SuspendFn`: a
    /// tuple type, or a type parameter bounded by `Tuple`
    /// (`fn.type.ctor.inputs-tuple`).
    fn inputs_tuple(&self, t: Ty) -> bool {
        match self.pool().get(t) {
            TyData::Tuple { .. } | TyData::Poison | TyData::Infer(_) => true,
            TyData::Param(_) => {
                let tuple = self.cx.names.known.tuple;
                (0..self.env.clause_self.len())
                    .any(|i| self.env.clause_self[i] == t && self.env.clause_trait[i] == tuple)
            }
            _ => false,
        }
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
        // An omitted trailing argument takes its default with the earlier
        // arguments substituted (`types.generic.default.written`).
        let mut args = hd_resolve::fill_type_args(self.cx.names.pool, def, &item.generics, args);
        while args.len() < n {
            args.push(self.infer.fresh(pool, VarKind::General));
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

    fn named_ty_as(&mut self, n: NodeRef<'_>, role: NameRole) -> StageResult<Ty> {
        let pool = self.pool();
        let segs = self.segments(n);
        let mut args = Vec::new();
        let mut arg_nodes = Vec::new();
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
                    arg_nodes.push(a);
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
            if name == "void" {
                return Ok(Ty::VOID);
            }
            if let Some(p) = Prim::ALL.iter().find(|p| p.name() == name) {
                return Ok(Ty::prim(*p));
            }
        }
        if self.is_poison_name(&segs[0]) {
            return Ok(Ty::POISON);
        }
        let Some(def) = self.resolve_path(&segs) else {
            // A single name that reaches nothing is `unknown-type`, or
            // `unknown-trait` where a trait is written (`req.key.in-scope`).
            if let [name] = segs.as_slice() {
                let (code, what) = match role {
                    NameRole::Type => (Code::UnknownType, "type"),
                    NameRole::Trait => (Code::UnknownTrait, "trait"),
                };
                self.err(code, n, &format!("no {what} named `{name}`"));
                return Ok(Ty::POISON);
            }
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
                let inputs = args.first().copied().unwrap_or(Ty::POISON);
                if !self.inputs_tuple(inputs) {
                    let msg = format!(
                        "the inputs of `{name}` are a tuple type or a type parameter bounded by `Tuple`, as in `{name}[(A, B), O, $()]`"
                    );
                    self.err(Code::GenericKindMismatch, n, &msg);
                    return Ok(Ty::POISON);
                }
                // Interning gives a tuple its parameter-per-element form,
                // a rest element the `List` it is (`fn.type.vararg-rest`).
                return Ok(pool.intern_ty(&TyData::Fn {
                    params: pool.list(&[inputs]),
                    result: args.get(1).copied().unwrap_or(Ty::VOID),
                    row: row.unwrap_or(RowId::EMPTY),
                    suspends: name == "SuspendFn",
                    inputs: Inputs::Tuple,
                }));
            }
        }
        // `types.generic.too-long`: no more arguments than parameters, in a
        // type, an alias or a trait alike.
        if let Some(item) = self.cx.lookup.item(def) {
            let max = item.generics.iter().filter(|g| !g.row).count();
            if args.len() > max {
                let msg = format!(
                    "`{}` takes {max} type arguments, not {}",
                    self.cx.names.display_name(def),
                    args.len()
                );
                self.err(Code::ArgumentCount, n, &msg);
                return Ok(Ty::POISON);
            }
        }
        if self.kind_of_item(def) == Some(HeadKind::Trait) {
            if role == NameRole::Type {
                // A bare trait name in a type position
                // (`types.trait.value.dyn-required`); the fix is `dyn`.
                let name = self.cx.names.display_name(def);
                let msg = format!("`{name}` is a trait, not a type");
                self.err(Code::TraitUsedAsType, n, &msg);
                let span = self.cx.src.span(n);
                let at = Span {
                    hi: span.lo,
                    ..span
                };
                self.diags.add_fix(
                    "insert `dyn` before the trait name",
                    hd_diag::FixSafety::Exact,
                    &[(at, "dyn ")],
                );
                return Ok(Ty::POISON);
            }
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
            // A binding names the associated type the trait declares or
            // reaches through its supertraits (trait.binding.name-reach),
            // as header lowering resolves it.
            let view = hd_resolve::assoc::LookupView {
                lookup: self.cx.lookup,
                names: &self.cx.names,
            };
            let (bindings, bad) =
                hd_resolve::assoc::resolve_bindings(pool, &view, (def, args, None), bound);
            for b in bad {
                let (code, msg) = b.diagnostic(self.cx.names.display_name(def));
                self.err(code, n, &msg);
            }
            let t = pool.intern_ty(&TyData::TraitValue {
                def,
                args,
                bindings,
            });
            return Ok(t);
        }
        // A written type infers nothing: a slot it omits needs a default
        // (`types.generic.default.written-missing`).
        if !self.in_explicit_targs
            && let Some(item) = self.cx.lookup.item(def)
        {
            let filled = hd_resolve::fill_type_args(self.cx.names.pool, def, &item.generics, &args);
            if let Some(g) = item
                .generics
                .iter()
                .skip(filled.len())
                .find(|g| !g.row && g.default.is_none())
            {
                let msg = format!(
                    "`{}` is written without its parameter `{}`, which has no default",
                    self.cx.names.display_name(def),
                    self.cx.names.text(g.name)
                );
                self.err(Code::PartialGenericArguments, n, &msg);
                return Ok(Ty::POISON);
            }
        }
        let t = self.ctor(def, &args)?;
        // Where a trait goes, only a row alias stands besides a trait: any
        // other type, a type alias among them, is no trait
        // (`req.key.traits`, `req.row.alias.type-alias.key`).
        if role == NameRole::Trait && !matches!(pool.get(t), TyData::Row(_) | TyData::Poison) {
            let msg = format!("no trait named `{}`: not a trait", segs.join("."));
            self.err(Code::UnknownTrait, n, &msg);
            return Ok(Ty::POISON);
        }
        self.check_written_bounds(t, def, (&arg_nodes, n))?;
        Ok(t)
    }

    /// A written type meets the declared bounds of its constructor's
    /// parameters (`types.generic.bound`, `types.map-key.declared-bound`):
    /// one error at the argument that misses a bound. A `mut` map key is
    /// `invalid-map-key` instead (`types.map-key.no-mut`), and an argument
    /// the written type did not fix waits for inference.
    fn check_written_bounds(
        &mut self,
        t: Ty,
        def: DefId,
        (arg_nodes, whole): (&[NodeRef<'_>], NodeRef<'_>),
    ) -> StageResult<()> {
        let pool = self.pool();
        let TyData::Adt { def: d, args } = pool.get(t) else {
            return Ok(());
        };
        if d != def {
            return Ok(());
        }
        let Some(item) = self.cx.lookup.item(def) else {
            return Ok(());
        };
        let args = pool.list_items(args);
        for (i, g) in item.generics.iter().enumerate() {
            let Some(&a) = args.get(i) else { continue };
            let at = arg_nodes.get(i).copied().unwrap_or(whole);
            if def == self.cx.names.known.map && i == 0 && matches!(pool.get(a), TyData::Mut(_)) {
                let msg = format!(
                    "{} is a `mut` map key type; a map key type is not `mut`",
                    self.show(a)
                );
                self.err(Code::InvalidMapKey, at, &msg);
                continue;
            }
            for b in &g.bounds {
                let b = pool.subst(*b, &|p: ParamRef| {
                    (p.owner == def)
                        .then(|| args.get(usize::from(p.index)).copied())
                        .flatten()
                });
                let TyData::TraitValue {
                    def: trait_,
                    args: targs,
                    ..
                } = pool.get(b)
                else {
                    continue;
                };
                let before = self.diags.len();
                let tref = hd_types::solver::TraitRef {
                    trait_,
                    self_ty: a,
                    args: targs,
                };
                self.require_ref(tref, at)?;
                if self.diags.len() != before {
                    break;
                }
            }
        }
        Ok(())
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
                let base = self.cx.scope_at(self.at).modules.get(m as usize)?.clone();
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
            let t = self.named_ty_as(c, NameRole::Trait)?;
            if self.inspectable_key(t, c) {
                continue;
            }
            self.require_complete(t, c);
            data.keys.push(t);
        }
        Ok(self.pool().row(&data))
    }

    /// Reports `t` written as a requirement key when it is `Inspectable`
    /// or has it as a supertrait (`req.key.inspectable.error`).
    fn inspectable_key(&mut self, t: Ty, at: NodeRef<'_>) -> bool {
        let pool = self.pool();
        let found = crate::body::is_inspectable_key(
            self.cx.lookup,
            pool,
            self.cx.names.known.inspectable,
            t,
        );
        if found {
            let msg = format!(
                "{} is `Inspectable` or extends it, so it cannot be a requirement key",
                self.show(t)
            );
            self.err(Code::InspectableRequirement, at, &msg);
        }
        found
    }

    /// A trait value type binds every associated type of its trait and
    /// supertraits (trait.dyn.binding.complete, req.key.binding.complete).
    fn require_complete(&mut self, t: Ty, at: NodeRef<'_>) {
        let view = hd_resolve::assoc::LookupView {
            lookup: self.cx.lookup,
            names: &self.cx.names,
        };
        if let Some(msg) = hd_resolve::assoc::incomplete_message(&self.cx.names, &view, t) {
            self.err(Code::TraitNotDynamicallySafe, at, &msg);
        }
    }

    /// The type a type node names.
    pub(crate) fn ty_node(&mut self, n: NodeRef<'_>) -> StageResult<Ty> {
        self.ty_node_as(n, NameRole::Type)
    }

    /// A type written in an explicit type-argument list of an expression:
    /// the expression infers the arguments it leaves out. In the slot of a
    /// row parameter (`row_slot`), a bare key is a kind error whose fix
    /// adds `$` (`fn.generic.explicit.row-dollar`).
    pub(crate) fn explicit_targ(&mut self, n: NodeRef<'_>, row_slot: bool) -> StageResult<Ty> {
        if row_slot
            && n.kind() == SyntaxKind::NamedType
            && Src::child(n, SyntaxKind::TypeArgumentList).is_none()
        {
            let segs = self.segments(n);
            let param = matches!(segs.as_slice(), [s] if self.type_param(s).is_some());
            if !param
                && self
                    .resolve_path(&segs)
                    .is_some_and(|d| self.kind_of_item(d) == Some(HeadKind::Trait))
            {
                let name = segs.join(".");
                let msg = format!(
                    "`{name}` is one key, but this slot takes a row: write it after `$`, as in `$ {name}`"
                );
                self.err(Code::GenericKindMismatch, n, &msg);
                let span = self.cx.src.span(n);
                let at = Span {
                    hi: span.lo,
                    ..span
                };
                self.diags.add_fix(
                    "insert `$` before the key",
                    hd_diag::FixSafety::Exact,
                    &[(at, "$ ")],
                );
                return Ok(Ty::POISON);
            }
        }
        let outer = std::mem::replace(&mut self.in_explicit_targs, true);
        let t = self.ty_node(n);
        self.in_explicit_targs = outer;
        t
    }

    /// The type a requirement key names: a trait.
    pub(crate) fn key_node(&mut self, n: NodeRef<'_>) -> StageResult<Ty> {
        let t = self.ty_node_as(n, NameRole::Trait)?;
        Ok(if self.inspectable_key(t, n) {
            Ty::POISON
        } else {
            t
        })
    }

    pub(crate) fn ty_node_as(&mut self, n: NodeRef<'_>, role: NameRole) -> StageResult<Ty> {
        let pool = self.pool();
        let first = |me: &mut Self, n: NodeRef<'_>| -> StageResult<Ty> {
            match n.children().find(|c| c.kind().is_type()) {
                Some(c) => me.ty_node(c),
                None => Ok(Ty::VOID),
            }
        };
        Ok(match n.kind() {
            SyntaxKind::NamedType => {
                let t = self.named_ty_as(n, role)?;
                // A row alias is row-kinded (`req.row.alias.type-or-key`).
                if matches!(pool.get(t), TyData::Row(_)) {
                    let name = self.segments(n).join(".");
                    let msg = format!(
                        "`{name}` is a row alias, not a type or one key: write it in a row after `$`, as in `$ {name}`"
                    );
                    self.err(hd_diag::Code::GenericKindMismatch, n, &msg);
                    return Ok(Ty::POISON);
                }
                t
            }
            SyntaxKind::ContextType => {
                let row = n.children().find(|c| c.kind().is_type()).filter(|c| {
                    c.kind() == SyntaxKind::RequirementRow
                        && c.direct_token(&self.cx.src.parse.tokens, TokenKind::Dollar)
                            .is_some()
                });
                let Some(row) = row else {
                    // The brackets are a row slot (`req.context.dollar`).
                    self.err(
                        hd_diag::Code::GenericKindMismatch,
                        n,
                        "`$.Context[...]` takes a row written after `$`",
                    );
                    return Ok(Ty::POISON);
                };
                let r = self.row_of(row)?;
                if !pool.row_data(r).params.is_empty() {
                    self.err(
                        hd_diag::Code::RowParameterInContext,
                        row,
                        "`$.Context[...]` takes only a concrete row, not a row parameter",
                    );
                    return Ok(Ty::POISON);
                }
                pool.intern_ty(&TyData::Context(r))
            }
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
                let mut rest = None;
                for c in n.children().filter(|c| c.kind().is_type()) {
                    if c.kind() == SyntaxKind::RestType {
                        let t = first(self, c)?;
                        // `types.tuple.rest.list`: a rest element is a `List`.
                        let list = self.cx.names.known.list;
                        if !matches!(pool.get(t), TyData::Adt { def, .. } if def == list)
                            && t != Ty::POISON
                        {
                            self.err(
                                hd_diag::Code::TypeMismatch,
                                c,
                                "a rest element's type must be a `List`",
                            );
                        }
                        rest = Some(t);
                    } else {
                        elems.push(self.ty_node(c)?);
                    }
                }
                pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&elems),
                    rest,
                })
            }
            SyntaxKind::DynType => match Src::child(n, SyntaxKind::NamedType) {
                Some(c) => {
                    let t = self.named_ty_as(c, NameRole::Trait)?;
                    let view = hd_resolve::assoc::LookupView {
                        lookup: self.cx.lookup,
                        names: &self.cx.names,
                    };
                    let t = hd_resolve::assoc::with_super_bindings(pool, &view, t);
                    self.require_complete(t, n);
                    t
                }
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
                let mut inputs = Inputs::Fixed;
                for c in n.children() {
                    if c.kind() == SyntaxKind::RequirementRow {
                        row = self.row_of(c)?;
                    } else if c.kind().is_type() {
                        let after = arrow.is_some_and(|a| self.cx.src.first(c).raw() > a);
                        let t = self.ty_node(c)?;
                        if after {
                            result = t;
                        } else {
                            inputs = if c.kind() == SyntaxKind::RestType {
                                Inputs::Rest
                            } else {
                                Inputs::Fixed
                            };
                            params.push(t);
                        }
                    }
                }
                pool.intern_ty(&TyData::Fn {
                    params: pool.list(&params),
                    result,
                    row,
                    suspends,
                    inputs,
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
                    // No bound declares the name: an associated-type
                    // error at the projection (`trait.assoc.projection`).
                    // A projection on a type with no bounds at all is
                    // still a gap.
                    let bounded =
                        (0..self.env.clause_self.len()).any(|i| self.env.clause_self[i] == base);
                    if bounded {
                        let msg =
                            format!("`{name}` names no associated type that any bound declares");
                        self.err(Code::UnknownAssociatedType, n, &msg);
                        return Ok(Ty::POISON);
                    }
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
