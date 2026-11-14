//! Type annotations inside bodies (`crates/hd_check/DESIGN.md` (type checking) §2.4): the same forms
//! as headers, read through the module scope, the item's type parameters
//! and the closure's interfaces. An `_` is a fresh variable.

use hd_base::{DefId, Span, StageResult};
use hd_diag::Code;
use hd_intern::PathKind;
use hd_resolve::{BindingKind, Generic, HeadKind, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{NONE, Ref, Tag, TirSink};
use hd_types::{Inputs, ParamRef, Prim, RowData, RowId, Ty, TyData, VarKind};

use crate::body::Ck;

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
    Referent,
    Trait,
}

impl Ck<'_, '_> {
    /// Checks the kinds of explicit arguments to a data, enum, trait or
    /// newtype declaration (`req.row.param.data-owner`,
    /// `req.row.slot.data`). Constructor inference supplies omitted slots;
    /// every written slot still has the declaration's type or row kind.
    /// A written argument of the wrong kind becomes poison, so nothing it
    /// fills reports again. The parameters a local type takes from around
    /// it have no written argument (`names.local-type.refs`).
    pub(crate) fn check_data_targ_kinds(
        &mut self,
        def: DefId,
        generics: &[Generic],
        args: &mut [Ty],
        n: NodeRef<'_>,
    ) {
        let pool = self.pool();
        let written = generics.iter().filter(|g| !g.outer);
        for (i, (arg, generic)) in args.iter_mut().zip(written).enumerate() {
            let data = pool.get(*arg);
            if matches!(data, TyData::Poison | TyData::Infer(_)) {
                continue;
            }
            let row = matches!(data, TyData::Row(_));
            if row == generic.row {
                continue;
            }
            let name = self.cx.names.display_name(def);
            let msg = if generic.row {
                format!(
                    "argument {} of `{name}` is a row parameter: write its row after `$`",
                    i + 1
                )
            } else {
                format!(
                    "argument {} of `{name}` is a type parameter, not a row",
                    i + 1
                )
            };
            self.err(Code::GenericKindMismatch, n, &msg);
            *arg = Ty::POISON;
        }
    }

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
    /// nothing when its declaration is broken, `private-import` when the
    /// module declares it without `pub` (`expr.name.qualified.private`),
    /// else `unknown-import` (`expr.name.qualified.missing`).
    pub(crate) fn missing_export(&mut self, module: u32, name: &str, n: NodeRef<'_>) {
        let path = self
            .cx
            .scope_at(self.at)
            .modules
            .get(module as usize)
            .cloned()
            .unwrap_or_default();
        let sym = self.cx.names.syms.intern(name);
        // A declaration whose source holds a parse diagnostic
        // (`grammar.recovery.item`): that error is the answer.
        if self
            .cx
            .lookup
            .ifaces
            .iter()
            .any(|f| f.broken_names.contains(&(path.clone(), sym)))
        {
            self.read_poison_name = true;
            return;
        }
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

    /// For a const slot of `def` at `position` among its non-lifetime
    /// parameters, its declared scalar type.
    fn const_slot_type(&self, def: DefId, position: usize) -> Option<Prim> {
        let item = self.cx.lookup.item(def)?;
        item.generics
            .iter()
            .filter(|g| !g.lifetime)
            .nth(position)
            .and_then(|g| g.const_type)
    }

    /// The kind of the slot of `def` at `position` among its non-lifetime
    /// parameters, as `generic-kind-mismatch` names it.
    fn slot_kind(&self, def: DefId, position: usize) -> &'static str {
        self.cx
            .lookup
            .item(def)
            .and_then(|item| item.generics.iter().filter(|g| !g.lifetime).nth(position))
            .map_or("type", Generic::kind_name)
    }

    /// Whether the slot of `def` at `position` among its written
    /// parameters takes a row (`req.row.slot.list`, `req.row.slot.data`).
    fn row_slot(&self, def: DefId, position: usize) -> bool {
        self.cx.lookup.item(def).is_some_and(|item| {
            item.generics
                .iter()
                .filter(|g| !g.lifetime && !g.outer)
                .nth(position)
                .is_some_and(|g| g.row)
        })
    }

    /// `a`, a type written without `$`, fills the row slot of `owner` at
    /// `position` (`req.row.slot.bare`). A bare name, a key or a row alias,
    /// gets the fix-it that writes `$` before it.
    fn bare_in_row_slot(&mut self, a: NodeRef<'_>, owner: &str, position: usize) {
        let msg = format!(
            "argument {} of `{owner}` is a row parameter: write its row after `$`",
            position + 1
        );
        self.err(Code::GenericKindMismatch, a, &msg);
        if a.kind() == SyntaxKind::NamedType {
            let span = self.cx.src.span(a);
            let at = Span {
                hi: span.lo,
                ..span
            };
            self.diags.add_fix(
                "write the row after `$`",
                hd_diag::FixSafety::Exact,
                &[(at, "$ ")],
            );
        }
    }

    /// A const argument of declared type `want`: its evaluated value or a
    /// bare const parameter (`types.generic.const.argument`).
    fn const_arg(&mut self, a: NodeRef<'_>, want: Option<Prim>) -> Ty {
        use hd_resolve::const_eval::{ConstArg, ConstName, const_argument};
        if a.kind() == SyntaxKind::NamedType {
            let segs = self.segments(a);
            if let [name] = segs.as_slice()
                && let Some(ty) = self.type_param(name)
                && let TyData::Param(p) = self.pool().get(ty)
                && self.generic_default_invisible(p, name, a)
            {
                return Ty::POISON;
            }
        }
        let src = self.cx.src;
        let syms = self.cx.names.syms;
        let paths = self.cx.names.paths;
        let tops = self.cx.init.borrow_mut().top_bindings(&src);
        let mut declared = None;
        let result = {
            // A named constant's initializer sees only the module's top
            // level: no parameter or local of the use (`in_binding`).
            let mut lookup = |name: &str, in_binding: bool| {
                let sym = syms.intern(name);
                if !in_binding
                    && let Some((_, p, prim)) =
                        self.const_gens.iter().rev().find(|(s, _, _)| *s == sym)
                {
                    declared = Some(*prim);
                    return ConstName::Param(*p);
                }
                if !in_binding && self.type_param(name).is_some() {
                    return ConstName::Type;
                }
                if !in_binding && self.find_local(sym).is_some() {
                    return ConstName::Runtime;
                }
                if let Some(b) = tops.get(&src, name) {
                    return b;
                }
                if Prim::ALL.iter().any(|p| p.name() == name) {
                    return ConstName::Type;
                }
                match self.scope_name(name) {
                    Some(Named::Item(d)) => {
                        if self
                            .cx
                            .lookup
                            .item(d)
                            .is_some_and(|item| item.sig().is_some())
                        {
                            ConstName::Runtime
                        } else {
                            ConstName::Type
                        }
                    }
                    Some(Named::Poison) => {
                        self.read_poison_name = true;
                        ConstName::Poison
                    }
                    _ if !in_binding
                        && hd_resolve::is_enclosing_parameter(&src, src.span(a).lo, name) =>
                    {
                        ConstName::Untaken
                    }
                    _ => ConstName::Unknown,
                }
            };
            const_argument(&src, a, want, paths, &mut lookup)
        };
        let pool = self.pool();
        match result {
            Ok(ConstArg::Value(v)) => pool.intern_ty(&TyData::Const(v)),
            Ok(ConstArg::Param(p)) => {
                if let (Some(want), Some(have)) = (want, declared)
                    && want != have
                {
                    let msg = format!(
                        "this const argument has type `{}`, but `{}` is expected",
                        have.name(),
                        want.name()
                    );
                    self.err(Code::TypeMismatch, a, &msg);
                    return Ty::POISON;
                }
                pool.intern_ty(&TyData::Param(p))
            }
            Ok(ConstArg::Poison) => Ty::POISON,
            Err(e) => {
                self.err(e.code, a, &e.message);
                Ty::POISON
            }
        }
    }

    /// `t`, named by `n` as `name`, which declares no generic parameters:
    /// any written type argument list is too long
    /// (`types.generic.too-long`), and the name is poisoned.
    fn takes_no_arguments(&mut self, n: NodeRef<'_>, name: &str, t: Ty) -> Ty {
        if Src::child(n, SyntaxKind::TypeArgumentList).is_none() {
            return t;
        }
        let msg = format!("`{name}` takes no type arguments");
        self.err(Code::ArgumentCount, n, &msg);
        Ty::POISON
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
                let env = &self.env;
                env.clauses_of(t)
                    .iter()
                    .any(|&i| env.clause_trait()[i as usize] == tuple)
            }
            _ => false,
        }
    }

    pub(crate) fn kind_of_item(&self, d: DefId) -> Option<HeadKind> {
        self.cx.lookup.item(d).and_then(hd_resolve::Item::kind)
    }

    /// A type constructor already selected by `named_ty_as`, the call
    /// dispatcher's type-kind arms, or the std known-item table. The
    /// driver's closure contains those declarations; failed headers may
    /// leave poison, as at other declaration reads.
    pub(crate) fn ctor(&mut self, def: DefId, args: &[Ty], n: NodeRef<'_>) -> Ty {
        let pool = self.pool();
        let Some(item) = self.cx.lookup.item(def) else {
            return Ty::POISON;
        };
        let args = self.ctor_args(item, args, n);
        match &item.data {
            ItemData::Alias { body, .. } => pool.subst(*body, &|p: ParamRef| {
                (p.owner == def)
                    .then(|| args.get(p.index as usize).copied())
                    .flatten()
            }),
            ItemData::Struct(_, _)
            | ItemData::Enum { .. }
            | ItemData::Newtype(_)
            | ItemData::Builtin => pool.intern_ty(&TyData::Adt {
                def,
                args: pool.list(&args),
            }),
            _ => Ty::POISON,
        }
    }

    /// `args` written for `item`, after the type, const and row parameters
    /// a local type takes from around it, each the one of the same name in
    /// scope here (`names.local-type.refs`, `req.row.param.scopes`).
    pub(crate) fn with_outer_args<'t>(
        &self,
        item: &hd_resolve::Item,
        args: &'t [Ty],
    ) -> std::borrow::Cow<'t, [Ty]> {
        let taken = item.generics.iter().take_while(|g| g.outer).count();
        if taken == 0 {
            return std::borrow::Cow::Borrowed(args);
        }
        item.generics[..taken]
            .iter()
            .map(|g| {
                if g.const_type.is_some() {
                    return self
                        .const_gens
                        .iter()
                        .rev()
                        .find(|c| c.0 == g.name)
                        .map_or(Ty::POISON, |(_, p, _)| {
                            self.pool().intern_ty(&TyData::Param(*p))
                        });
                }
                if g.row {
                    let pool = self.pool();
                    return self.row_gens.iter().rev().find(|r| r.0 == g.name).map_or(
                        Ty::POISON,
                        |(_, p)| {
                            let row = pool.row(&hd_types::RowData {
                                keys: vec![],
                                params: vec![*p],
                            });
                            pool.intern_ty(&TyData::Row(row))
                        },
                    );
                }
                // The enclosing trait's `Self` a local type takes is a
                // parameter named `Self` in a local impl of that type, whose
                // own `Self` is the local type.
                let name = self.cx.names.text(g.name);
                (name == "Self")
                    .then(|| self.gens.iter().rev().find(|(s, _)| *s == g.name))
                    .flatten()
                    .map(|(_, t)| *t)
                    .or_else(|| self.type_param(name))
                    .unwrap_or(Ty::POISON)
            })
            .chain(args.iter().copied())
            .collect()
    }

    /// Reports `item`'s first parameter from around it that is not in scope
    /// at `n`: one a local impl around `n` does not take
    /// (`names.local-type.refs.impl`). `with_outer` is
    /// [`Self::with_outer_args`]'s result, whose leading arguments are
    /// those parameters, poison where one is not in scope.
    pub(crate) fn report_untaken_outer(
        &mut self,
        item: &hd_resolve::Item,
        with_outer: &[Ty],
        n: NodeRef<'_>,
    ) {
        let Some(g) = item
            .generics
            .iter()
            .take_while(|g| g.outer)
            .zip(with_outer)
            .find_map(|(g, t)| (*t == Ty::POISON).then_some(g))
        else {
            return;
        };
        let msg = format!(
            "`{}` takes the enclosing parameter `{}`, which this local impl does not take; a local impl names an enclosing parameter only through its local target or trait",
            self.cx.names.display_name(item.def),
            self.cx.names.text(g.name)
        );
        self.err(Code::LocalImplOuterParameter, n, &msg);
    }

    /// The arguments of the selected declaration: for a local type, the
    /// type parameters it takes from around it, each the one of the same
    /// name in scope here (`names.local-type.refs`); then those written,
    /// then each omitted one's default or a fresh variable.
    pub(crate) fn ctor_args(
        &mut self,
        item: &hd_resolve::Item,
        args: &[Ty],
        n: NodeRef<'_>,
    ) -> Vec<Ty> {
        let pool = self.pool();
        let def = item.def;
        let with_outer = self.with_outer_args(item, args);
        self.report_untaken_outer(item, &with_outer, n);
        let args = with_outer.as_ref();
        // An omitted trailing argument takes its default with the earlier
        // arguments substituted (`types.generic.default.written`).
        let mut args = hd_resolve::fill_type_args(self.cx.names.pool, def, &item.generics, args);
        while let Some(g) = item.generics.get(args.len()) {
            // `ref.lifetime.struct`: a lifetime has its one argument.
            args.push(if g.lifetime {
                hd_resolve::lifetime_arg(pool)
            } else {
                self.infer.fresh(pool, VarKind::General)
            });
        }
        self.row_argument_vars(def, &args, n);
        args
    }

    fn named_ty_as(&mut self, n: NodeRef<'_>, role: NameRole) -> StageResult<Ty> {
        let pool = self.pool();
        let segs = self.segments(n);
        let mut args = Vec::new();
        let mut arg_nodes = Vec::new();
        let mut row = None;
        // A trait value's associated-type bindings (`dyn Supplier[Item = i32]`).
        let mut bound = Vec::new();
        // An opaque type in a type argument of a list, a map or a trait
        // borrows nothing (`types.opaque.borrows.position`).
        let borrows = self.opaque_annotation.map(|(_, b)| b);
        if borrows == Some(true)
            && let Some(def) = self.resolve_path(&segs)
        {
            let known = &self.cx.names.known;
            if def == known.list
                || def == known.map
                || self.kind_of_item(def) == Some(HeadKind::Trait)
            {
                self.opaque_annotation = self.opaque_annotation.map(|(s, _)| (s, false));
            }
        }
        // The constructor, for its const slots (`types.generic.const.argument`).
        let ctor = self.resolve_path(&segs);
        let mut position = 0_usize;
        if let Some(al) = Src::child(n, SyntaxKind::TypeArgumentList) {
            // ref.lifetime.not-yet.body: a lifetime argument in a body.
            let toks = &self.cx.src.parse.tokens;
            if al
                .direct_tokens()
                .any(|t| toks.kind(t) == hd_syntax::TokenKind::Lifetime)
            {
                self.err(
                    Code::ReferencesNotYetChecked,
                    n,
                    "a lifetime inside a function body is not checked yet",
                );
            }
            for a in al.children() {
                if a.kind() == SyntaxKind::RequirementRow {
                    let r = self.row_of(a)?;
                    row = Some(r);
                    args.push(pool.intern_ty(&TyData::Row(r)));
                    arg_nodes.push(a);
                    position += 1;
                } else if let Some(want) = ctor
                    .filter(|_| a.kind() == SyntaxKind::ConstArgument || a.kind().is_type())
                    .and_then(|d| self.const_slot_type(d, position))
                {
                    args.push(self.const_arg(a, Some(want)));
                    arg_nodes.push(a);
                    position += 1;
                } else if a.kind() == SyntaxKind::ConstArgument {
                    // An unknown constructor is its own error, reported
                    // with the name below.
                    if let Some(def) = ctor {
                        let slot = self.slot_kind(def, position);
                        let msg = hd_resolve::generic_kind_mismatch(
                            position,
                            &segs.join("."),
                            slot,
                            "a const expression",
                        );
                        self.err(Code::GenericKindMismatch, a, &msg);
                    }
                    args.push(Ty::POISON);
                    arg_nodes.push(a);
                    position += 1;
                } else if a.kind() == SyntaxKind::AssociatedTypeBinding {
                    if let (Some(t), Some(ty)) =
                        (a.name(&self.cx.src.parse.tokens), Src::type_child(a))
                    {
                        let name = self.cx.src.text(t).to_owned();
                        bound.push((name, self.ty_node(ty)?));
                    }
                } else if a.kind().is_type() && ctor.is_some_and(|d| self.row_slot(d, position)) {
                    // A row slot takes its row after `$` (`req.row.slot.bare`):
                    // a bare name there is that one kind error, before it
                    // can be lowered, and misread, as a type.
                    self.bare_in_row_slot(a, &segs.join("."), position);
                    args.push(Ty::POISON);
                    arg_nodes.push(a);
                    position += 1;
                } else if a.kind().is_type() {
                    args.push(self.ty_node(a)?);
                    arg_nodes.push(a);
                    position += 1;
                }
            }
        }
        if role == NameRole::Trait
            && segs.len() == 1
            && (segs[0] == "Fn" || segs[0] == "FnMut")
            && n.direct_token(&self.cx.src.parse.tokens, TokenKind::Arrow)
                .is_some()
        {
            let mut tys = n
                .children()
                .filter(|c| c.kind().is_type() && c.kind() != SyntaxKind::RequirementRow);
            if let Some(input) = tys.next() {
                args.push(self.ty_node(input)?);
                arg_nodes.push(input);
            }
            if let Some(output) = tys.next() {
                args.push(self.ty_node(output)?);
                arg_nodes.push(output);
            }
            let (r, node) = match Src::child(n, SyntaxKind::RequirementRow) {
                Some(r) => (self.row_of(r)?, r),
                None => (RowId::EMPTY, n),
            };
            row = Some(r);
            args.push(pool.intern_ty(&TyData::Row(r)));
            arg_nodes.push(node);
        }
        if let (Some((s, _)), Some(b)) = (self.opaque_annotation, borrows) {
            self.opaque_annotation = Some((s, b));
        }
        if segs.len() == 1 {
            let name = segs[0].as_str();
            if self
                .const_gens
                .iter()
                .any(|(s, _, _)| self.cx.names.text(*s) == name)
            {
                // `types.generic.const.type-position`: only a const slot.
                let msg = format!(
                    "`{name}` is a const parameter, not a type: write it in a const generic argument"
                );
                self.err(Code::GenericKindMismatch, n, &msg);
                return Ok(Ty::POISON);
            }
            if role == NameRole::Trait && (name == "Self" || self.type_param(name).is_some()) {
                // A type parameter names a type, never a trait
                // (`req.key.traits`), as a signature reports it.
                let msg = format!("no trait named `{name}`");
                self.err(Code::UnknownTrait, n, &msg);
                return Ok(Ty::POISON);
            }
            if let Some(t) = self.type_param(name) {
                if let TyData::Param(p) = pool.get(t)
                    && self.generic_default_invisible(p, name, n)
                {
                    return Ok(Ty::POISON);
                }
                if let TyData::Param(p) = pool.get(t)
                    && self
                        .row_gens
                        .iter()
                        .any(|(_, r)| r.owner == p.owner && r.index == p.index)
                {
                    let msg = format!(
                        "`{name}` is a row parameter, not a type; write it inside a row after `$`"
                    );
                    self.err(Code::GenericKindMismatch, n, &msg);
                    return Ok(Ty::POISON);
                }
                return Ok(self.takes_no_arguments(n, name, t));
            }
            let builtin = match name {
                "never" => Some(Ty::NEVER),
                "void" => Some(Ty::VOID),
                _ => Prim::ALL
                    .iter()
                    .find(|p| p.name() == name)
                    .map(|p| Ty::prim(*p)),
            };
            if let Some(t) = builtin {
                // A primitive, `void` or `never` is no trait (`req.key.traits`).
                if role == NameRole::Trait {
                    let msg = format!("no trait named `{name}`: not a trait");
                    self.err(Code::UnknownTrait, n, &msg);
                    return Ok(Ty::POISON);
                }
                return Ok(self.takes_no_arguments(n, name, t));
            }
        }
        if self.is_poison_name(&segs[0]) {
            self.read_poison_name = true;
            return Ok(Ty::POISON);
        }
        let Some(def) = self.resolve_path(&segs) else {
            // A single name that reaches nothing is `unknown-type`, or
            // `unknown-trait` where a trait is written (`req.key.in-scope`).
            if let [name] = segs.as_slice() {
                if hd_resolve::is_enclosing_parameter(&self.cx.src, self.cx.src.span(n).lo, name) {
                    let msg = format!(
                        "`{name}` is an enclosing parameter this local impl does not take; a local impl names an enclosing parameter only through its local target or trait"
                    );
                    self.err(Code::LocalImplOuterParameter, n, &msg);
                    return Ok(Ty::POISON);
                }
                let (code, what) = match role {
                    NameRole::Type | NameRole::Referent => (Code::UnknownType, "type"),
                    NameRole::Trait => (Code::UnknownTrait, "trait"),
                };
                self.err(
                    code,
                    n,
                    &format!("no {what} named `{name}`{}", self.import_hint(name)),
                );
                return Ok(Ty::POISON);
            }
            // `names.type.qualified`: through a module in scope, a missing
            // declaration is `unknown-import`; a first name that is no
            // module is `unknown-name`, as in a signature.
            if let (Some(Named::Module(m)), [_, name]) = (self.scope_name(&segs[0]), &segs[..]) {
                self.missing_export(m, name, n);
            } else {
                let msg = format!("`{}` is not defined", segs[0]);
                self.err(Code::UnknownName, n, &msg);
            }
            return Ok(Ty::POISON);
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
            if role != NameRole::Trait && (name == "Fn" || name == "SuspendFn") {
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
                    deps: hd_types::FnDeps::NONE,
                }));
            }
        }
        // `types.generic.too-long`: no more arguments than parameters, in a
        // type, an alias or a trait alike.
        if let Some(item) = self.cx.lookup.item(def) {
            // The parameters a local type takes from around it have no
            // written argument (`names.local-type.refs`).
            let max = item
                .generics
                .iter()
                .filter(|g| !g.lifetime && !g.outer)
                .count();
            if args.len() > max {
                let msg = format!(
                    "`{}` takes {max} type arguments, not {}",
                    self.cx.names.display_name(def),
                    args.len()
                );
                self.err(Code::ArgumentCount, n, &msg);
                return Ok(Ty::POISON);
            }
            // A row slot's argument is a row or, once reported where it is
            // written, poison (`bare_in_row_slot`); only a row in a type
            // slot is left to report.
            if let Some(i) = args
                .iter()
                .zip(item.generics.iter().filter(|g| !g.outer))
                .position(|(a, g)| !g.row && matches!(pool.get(*a), TyData::Row(_)))
            {
                let msg = format!(
                    "argument {} of `{}` is a type parameter, not a row",
                    i + 1,
                    self.cx.names.display_name(def)
                );
                self.err(Code::GenericKindMismatch, n, &msg);
                return Ok(Ty::POISON);
            }
        }
        if matches!(self.kind_of_item(def), Some(HeadKind::Fn | HeadKind::Impl)) {
            // A function's name reaches no type (`unknown-type`).
            let (code, what) = match role {
                NameRole::Type | NameRole::Referent => (Code::UnknownType, "type"),
                NameRole::Trait => (Code::UnknownTrait, "trait"),
            };
            let name = self.cx.names.display_name(def);
            self.err(code, n, &format!("no {what} named `{name}`: not a {what}"));
            return Ok(Ty::POISON);
        }
        if self.kind_of_item(def) == Some(HeadKind::Trait) {
            if role != NameRole::Trait {
                // A bare trait name in a type position
                // (`types.trait.value.dyn-required.reference-values`); the fix is `dyn`.
                let name = self.cx.names.display_name(def);
                let msg = format!("`{name}` is a trait, not a type");
                self.err(Code::TraitUsedAsType, n, &msg);
                let span = self.cx.src.span(n);
                let at = Span {
                    hi: span.lo,
                    ..span
                };
                let (title, prefix) = if role == NameRole::Referent {
                    ("insert `dyn` before the trait name", "dyn ")
                } else {
                    ("insert `&dyn` before the trait name", "&dyn ")
                };
                self.diags
                    .add_fix(title, hd_diag::FixSafety::Exact, &[(at, prefix)]);
                return Ok(Ty::POISON);
            }
            // A written trait value omits its defaulted trailing arguments
            // (`types.generic.default.written`); no `Self` is known here.
            // A local trait's parameters from around it come first, each the
            // one of the same name in scope (`names.local-type.refs`).
            let args = match self.cx.lookup.item(def) {
                Some(it) => {
                    let args = self.with_outer_args(it, &args).into_owned();
                    self.report_untaken_outer(it, &args, n);
                    hd_resolve::fill_trait_args(pool, def, &it.generics, pool.list(&args), None)
                }
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
            let with_outer = self.with_outer_args(item, &args);
            self.report_untaken_outer(item, &with_outer, n);
            let filled =
                hd_resolve::fill_type_args(self.cx.names.pool, def, &item.generics, &with_outer);
            if let Some(g) = item
                .generics
                .iter()
                .skip(filled.len())
                .find(|g| g.default.is_none())
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
        let t = self.ctor(def, &args, n);
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
    /// parameters, their bindings among them (`types.generic.bound`,
    /// `types.map-key.declared-bound`, `trait.bound.no-implied.positions`):
    /// one error at the argument that misses a bound. An argument the
    /// written type did not fix waits for inference.
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
        let mut missed = None;
        for (i, arg, tref, bound) in crate::written_bounds::goals(pool, def, &item.generics, args) {
            if missed == Some(i) {
                continue;
            }
            let at = arg_nodes.get(i).copied().unwrap_or(whole);
            let before = self.diags.len();
            self.require_ref(tref, at)?;
            if self.diags.len() == before {
                self.check_bindings(arg, bound, at)?;
            }
            if self.diags.len() != before {
                missed = Some(i);
            }
        }
        Ok(())
    }

    /// `Map[K < Eq & Hash, V]` (types.map-key.declared-bound): one error
    /// for a key type that misses either trait.
    pub(crate) fn check_map_key(&mut self, k: Ty, at: NodeRef<'_>) -> StageResult<()> {
        let before = self.diags.len();
        for tr in [self.cx.names.known.eq, self.cx.names.known.hash] {
            let tref = hd_types::solver::TraitRef {
                trait_: tr,
                self_ty: k,
                args: hd_types::TyList::EMPTY,
            };
            self.require_ref(tref, at)?;
            if self.diags.len() != before {
                break;
            }
        }
        Ok(())
    }

    /// `t` is `Copy`: a list or map literal's item type, as a collection
    /// holds copyable items (`ref.copy.collections`), or a constructed
    /// type's argument.
    pub(crate) fn check_collection_item(&mut self, t: Ty, at: NodeRef<'_>) -> StageResult<()> {
        let tref = hd_types::solver::TraitRef {
            trait_: self.cx.names.known.copy,
            self_ty: t,
            args: hd_types::TyList::EMPTY,
        };
        self.require_ref_as(tref, at, Code::UnsatisfiedTraitBound)?;
        Ok(())
    }

    /// The type arguments of a struct literal or variant construction of
    /// `def` meet its parameters' declared bounds and their bindings, as a
    /// written `def[..]` does (`trait.bound.unsatisfied`,
    /// `trait.bound.no-implied.positions`), the default `Copy` bound among
    /// them (`ref.copy.generic.default`): one error per argument, at the
    /// construction. An argument still open waits for inference.
    pub(crate) fn check_construct_bounds(
        &mut self,
        def: DefId,
        args: &[Ty],
        at: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let Some(item) = self.cx.lookup.item(def) else {
            return Ok(());
        };
        let mut missed = None;
        for (i, arg, tref, bound) in crate::written_bounds::goals(pool, def, &item.generics, args) {
            if missed == Some(i) {
                continue;
            }
            let before = self.diags.len();
            self.require_ref_as(tref, at, Code::UnsatisfiedTraitBound)?;
            if self.diags.len() == before {
                self.check_bindings(arg, bound, at)?;
            }
            if self.diags.len() != before {
                missed = Some(i);
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

    /// `types.generic.default.later`: a local default sees only the
    /// parameters before its own slot.
    fn generic_default_invisible(&mut self, p: ParamRef, name: &str, n: NodeRef<'_>) -> bool {
        let blocked = p.owner == self.gen_owner
            && self
                .generic_default_from
                .is_some_and(|from| p.index >= from);
        if blocked {
            let msg = format!(
                "`{name}` is not visible here: a generic default names only earlier parameters"
            );
            self.err(Code::BindingNotYetVisible, n, &msg);
        }
        blocked
    }

    /// The declaration of a function or local function parameter, found
    /// among only its active lexical scopes (`req.row.param.unmarked`).
    fn generic_source(&self, p: ParamRef) -> Option<NodeRef<'_>> {
        let node = self.generic_sources.iter().rev().find_map(|scope| {
            let position = usize::from(p.index.checked_sub(scope.first)?);
            (scope.owner == p.owner)
                .then(|| scope.params.get(position).copied())
                .flatten()
        })?;
        Some(self.cx.src.parse.tree.node(node))
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
                let p = *p;
                if !self.generic_default_invisible(
                    ParamRef {
                        owner: p.owner,
                        index: p.index,
                    },
                    name,
                    c,
                ) {
                    data.params.push(p);
                }
                continue;
            }
            if let [name] = segs.as_slice()
                && let Some(t) = self.type_param(name)
                && let TyData::Param(p) = self.pool().get(t)
                && !self.const_gens.iter().any(|(_, c, _)| *c == p)
            {
                if !self.generic_default_invisible(p, name, c) {
                    let msg = format!(
                        "`{name}` is a type parameter, not a row parameter: declare it as `${name}`"
                    );
                    self.err(Code::GenericKindMismatch, c, &msg);
                    if let Some(declaration) = self.generic_source(p) {
                        let span = self.cx.src.span(declaration);
                        self.diags.add_fix(
                            "mark the parameter as a row parameter",
                            hd_diag::FixSafety::Exact,
                            &[(
                                Span {
                                    hi: span.lo,
                                    ..span
                                },
                                "$",
                            )],
                        );
                    }
                }
                continue;
            }
            let t = self.named_ty_as(c, NameRole::Trait)?;
            if self.inspectable_key(t, c) {
                continue;
            }
            self.require_complete(t, c);
            let t = self.with_key_bindings(t);
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
    /// supertraits (trait.dyn.binding.complete.reference-values, req.key.binding.complete).
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
        let t = self.ty_node_as(n, NameRole::Type)?;
        // A written type reports its local references itself
        // (`local_refs::declarations`).
        self.declared_tys.insert(t);
        Ok(t)
    }

    /// The written arguments of `tl` for the parameters `gens`: a const
    /// slot's argument evaluated (`types.generic.const.argument`), any
    /// other lowered as a type.
    pub(crate) fn explicit_targs(
        &mut self,
        owner: NodeRef<'_>,
        tl: NodeRef<'_>,
        gens: &[Generic],
    ) -> StageResult<Vec<Ty>> {
        let mut out = Vec::new();
        for (i, t) in tl
            .children()
            .filter(|c| c.kind().is_type() || c.kind() == SyntaxKind::ConstArgument)
            .enumerate()
        {
            out.push(self.explicit_slot_arg(owner, t, gens.get(i), i)?);
        }
        Ok(out)
    }

    /// One written argument `t` at `position` for the parameter `g` of the
    /// item `owner` names.
    pub(crate) fn explicit_slot_arg(
        &mut self,
        owner: NodeRef<'_>,
        t: NodeRef<'_>,
        g: Option<&Generic>,
        position: usize,
    ) -> StageResult<Ty> {
        if let Some(prim) = g.and_then(|g| g.const_type) {
            return Ok(self.const_arg(t, Some(prim)));
        }
        if t.kind() == SyntaxKind::ConstArgument {
            let msg = hd_resolve::generic_kind_mismatch(
                position,
                self.cx.src.node_text(owner),
                g.map_or("type", Generic::kind_name),
                "a const expression",
            );
            self.err(Code::GenericKindMismatch, t, &msg);
            return Ok(Ty::POISON);
        }
        self.explicit_targ(t, g.is_some_and(|g| g.row))
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

    /// A key with the bindings its trait's supertrait lists fix added, so
    /// `$ PriceFeed` and `$ PriceFeed[Item = i32]` are one key
    /// (req.key.binding.identity.super).
    fn with_key_bindings(&self, t: Ty) -> Ty {
        let view = hd_resolve::assoc::LookupView {
            lookup: self.cx.lookup,
            names: &self.cx.names,
        };
        hd_resolve::assoc::with_super_bindings(self.pool(), &view, t)
    }

    /// The trait value type a requirement key names, with its trait;
    /// `None` once an error is reported. A key that names no trait, such
    /// as `i32`, is `unknown-trait`.
    pub(crate) fn key_node(&mut self, n: NodeRef<'_>) -> StageResult<Option<(Ty, DefId)>> {
        let t = self.ty_node_as(n, NameRole::Trait)?;
        if t == Ty::POISON || self.inspectable_key(t, n) {
            return Ok(None);
        }
        let t = self.with_key_bindings(t);
        if let TyData::TraitValue { def, .. } = self.pool().get(t) {
            return Ok(Some((t, def)));
        }
        let msg = format!("no trait named `{}`: not a trait", self.show(t));
        self.err(hd_diag::Code::UnknownTrait, n, &msg);
        Ok(None)
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
                pool.applied(self.cx.names.known.option, t)
            }
            SyntaxKind::UnionType => {
                let members: StageResult<Vec<Ty>> = n
                    .children()
                    .filter(|c| c.kind().is_type())
                    .map(|c| self.ty_node(c))
                    .collect();
                pool.intern_ty(&TyData::Union(pool.list(&members?)))
            }
            SyntaxKind::ParenType => match n.children().find(|c| c.kind().is_type()) {
                Some(child) => self.ty_node_as(child, role)?,
                None => Ty::VOID,
            },
            SyntaxKind::RestType => {
                let t = first(self, n)?;
                // Tuple types and function inputs use the same rest
                // element (`types.tuple.rest.list`, `fn.type.vararg-rest`).
                let list = self.cx.names.known.list;
                if !matches!(pool.get(t), TyData::Adt { def, .. } if def == list) && t != Ty::POISON
                {
                    self.err(
                        Code::TypeMismatch,
                        n,
                        "a rest element's type must be a `List`",
                    );
                }
                t
            }
            SyntaxKind::TupleType => {
                let mut elems = Vec::new();
                let mut rest = None;
                for c in n.children().filter(|c| c.kind().is_type()) {
                    let t = self.ty_node(c)?;
                    if c.kind() == SyntaxKind::RestType {
                        rest = Some(t);
                    } else {
                        elems.push(t);
                    }
                }
                pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&elems),
                    rest,
                })
            }
            // A local function's parameter annotation names its anonymous
            // type parameter; any other `impl` is an opaque result.
            SyntaxKind::ImplType => match self.impl_params.iter().find(|(i, _)| *i == n.index()) {
                Some(&(_, t)) => t,
                None => self.declared_opaque_annotation(n)?,
            },
            SyntaxKind::DynType => {
                if role != NameRole::Referent {
                    self.err(
                        Code::TraitUsedAsType,
                        n,
                        "a trait value needs a reference type, `&dyn` or `&mut dyn`",
                    );
                    return Ok(Ty::POISON);
                }
                let c = Src::child(n, SyntaxKind::NamedType)
                    .expect("parser::ty_bound_trait emits NamedType");
                let t = self.named_ty_as(c, NameRole::Trait)?;
                if let TyData::TraitValue { def, .. } = pool.get(t)
                    && let Some(name) = self.cx.names.known.fn_trait_name(def)
                {
                    let msg = format!(
                        "`{name}` is a static-dispatch function trait and has no `dyn` form; use a `fn(...) -> ...` type for dynamic calls"
                    );
                    self.err(Code::TraitNotDynamicallySafe, n, &msg);
                    return Ok(Ty::POISON);
                }
                let view = hd_resolve::assoc::LookupView {
                    lookup: self.cx.lookup,
                    names: &self.cx.names,
                };
                let t = hd_resolve::assoc::with_super_bindings(pool, &view, t);
                self.require_complete(t, n);
                t
            }
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
                // `fn.type.lifetime.elide`: a body writes no lifetime.
                let deps = pool.fn_deps_written(&params, result, &[]);
                pool.intern_ty(&TyData::Fn {
                    params: pool.list(&params),
                    result,
                    row,
                    suspends,
                    inputs,
                    deps,
                })
            }
            SyntaxKind::InferType => {
                let v = self.infer.fresh(pool, VarKind::General);
                self.row_argument_vars(DefId::NONE, &[v], n);
                v
            }
            // A row in a row slot, as an explicit argument for a row
            // parameter (`req.row.slot.list`).
            SyntaxKind::RequirementRow => {
                let r = self.row_of(n)?;
                pool.intern_ty(&TyData::Row(r))
            }
            SyntaxKind::ProjectionType => {
                let base = first(self, n)?;
                self.add_projection_clauses(base);
                let t = n.name_part(&self.cx.src.parse.tokens);
                let name = self.cx.src.text(t).to_owned();
                let Some((trait_, args)) = self.assoc_owner(base, &name) else {
                    // No bound declares the name: an associated-type
                    // error at the projection (`trait.assoc.no-type`). A
                    // type with no bound, a concrete one included,
                    // selects none either.
                    // A bound that names no trait was reported, and
                    // left a clause on no trait.
                    let env = &self.env;
                    let clauses = || env.clauses_of(base).iter().map(|&i| i as usize);
                    if base == Ty::POISON || clauses().any(|i| env.clause_trait()[i] == DefId::NONE)
                    {
                        return Ok(Ty::POISON);
                    }
                    let bounded = clauses().next().is_some();
                    let msg = if bounded {
                        format!("`{name}` names no associated type that any bound declares")
                    } else {
                        format!("`{name}` projects from a type with no bound that declares it")
                    };
                    self.err(Code::UnknownAssociatedType, n, &msg);
                    return Ok(Ty::POISON);
                };
                let assoc = self.cx.names.member(trait_, PathKind::Member, &name);
                self.normalize(pool.intern_ty(&TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty: base,
                    args,
                }))?
            }
            SyntaxKind::RefType => {
                // ref.lifetime.not-yet.body: a lifetime written in a body.
                if n.direct_token(&self.cx.src.parse.tokens, hd_syntax::TokenKind::Lifetime)
                    .is_some()
                {
                    self.err(
                        Code::ReferencesNotYetChecked,
                        n,
                        "a lifetime inside a function body is not checked yet",
                    );
                }
                // `ref.type.forms`: `&mut T` is the reference's permission.
                let mutable = n
                    .direct_token(&self.cx.src.parse.tokens, hd_syntax::TokenKind::KwMut)
                    .is_some();
                let inner = match n.children().find(|c| c.kind().is_type()) {
                    Some(child) => self.ty_node_as(child, NameRole::Referent)?,
                    None => Ty::VOID,
                };
                // `ref.type.counted-forms`: `*T` is counted, `&T` local.
                let counted = n
                    .direct_token(&self.cx.src.parse.tokens, hd_syntax::TokenKind::Star)
                    .is_some();
                pool.intern_ty(&TyData::Ref {
                    inner,
                    mutable,
                    counted,
                })
            }
            // `parser::types` emits only the type production kinds above
            // (`SyntaxKind::is_type`); malformed types stop before bodies.
            _ => Ty::POISON,
        })
    }
}
