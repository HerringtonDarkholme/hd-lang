//! Decorator target checking (annotations §Target Kinds, §Error Types).
//!
//! A header check: a decorator's fact type is the result type of the
//! function it calls, which is in the function's signature, and the type's
//! `@annotate(...)` line is declaration metadata. Neither needs a body, so
//! the check runs once the folder's items are lowered, with no checker run.

use std::collections::HashMap;

use hd_base::{DefId, Span};
use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_types::{Ty, TyData};

use super::Lower;
use crate::iface::{Field, HeadKind, Item, ItemData};
use crate::view::Src;
use crate::{Binding, BindingKind};

/// The target kinds, one bit each (`annot.target.kind.*`).
pub(crate) mod kind {
    pub const FN: u16 = 1 << 0;
    pub const DATA: u16 = 1 << 1;
    pub const ENUM: u16 = 1 << 2;
    pub const NEWTYPE: u16 = 1 << 3;
    pub const FIELD: u16 = 1 << 4;
    pub const VARIANT: u16 = 1 << 5;
    pub const PARAM: u16 = 1 << 6;
    pub const TRAIT: u16 = 1 << 7;
    pub const IMPL: u16 = 1 << 8;
    pub const METHOD: u16 = 1 << 9;
}

fn kind_bit(name: &str) -> Option<u16> {
    Some(match name {
        "Fn" => kind::FN,
        "Data" => kind::DATA,
        "Enum" => kind::ENUM,
        "Newtype" => kind::NEWTYPE,
        "Field" => kind::FIELD,
        "Variant" => kind::VARIANT,
        "Param" => kind::PARAM,
        "Trait" => kind::TRAIT,
        "Impl" => kind::IMPL,
        "Method" => kind::METHOD,
        _ => return None,
    })
}

/// One decorator line: the called or named function and its form.
struct Deco<'t> {
    name: String,
    bare: bool,
    typed: bool,
    args: Option<NodeRef<'t>>,
}

/// What surrounds a target, for the `@error` and `@from` forms.
#[derive(Clone, Copy, Default)]
struct Place {
    kind: u16,
    /// The enclosing declaration is an error type.
    error_type: bool,
    /// The number of members in the target's own list (payload or fields).
    members: usize,
    /// The member's resolved type, when it has one.
    ty: Option<Ty>,
}

impl Lower<'_, '_, '_> {
    fn deco<'t>(&self, d: NodeRef<'t>) -> Option<Deco<'t>> {
        let e = d.children().next()?;
        let (mut callee, args) = match e.kind() {
            SyntaxKind::CallExpr => (
                e.children().next()?,
                Src::child(e, SyntaxKind::ArgumentList),
            ),
            _ => (e, None),
        };
        let mut typed = false;
        if callee.kind() == SyntaxKind::TypeArgsExpr {
            typed = true;
            callee = callee.children().next()?;
        }
        if callee.kind() != SyntaxKind::NameExpr {
            return None;
        }
        let name = self.src.text(self.src.first_ident(callee)?).to_owned();
        Some(Deco {
            name,
            bare: e.kind() != SyntaxKind::CallExpr,
            typed,
            args,
        })
    }

    /// The item a name in this module's scope reaches.
    fn scope_item(&self, name: &str, items: &HashMap<DefId, &Item>) -> Option<(DefId, Item)> {
        let Some(Binding {
            kind: BindingKind::Item,
            value,
        }) = self.scope.lookup(self.sym(name))
        else {
            return None;
        };
        let d = DefId::from_raw(value);
        let item = items
            .get(&d)
            .map(|i| (*i).clone())
            .or_else(|| self.r.item(d))?;
        Some((d, item))
    }

    /// The target kinds that a data type's or enum's `@annotate(...)` line
    /// lists (`annot.target.limit`); a typed fact type attaches only to a
    /// field or a function (`annot.typed-fact.targets`).
    pub(super) fn annotate_mask(&self, n: NodeRef<'_>) -> Option<u16> {
        let mut mask = None;
        for d in n.children().filter(|c| c.kind() == SyntaxKind::Decorator) {
            let Some(deco) = self.deco(d) else { continue };
            if deco.name != "annotate" {
                continue;
            }
            let Some(Binding {
                kind: BindingKind::Item,
                value,
            }) = self.scope.lookup(self.sym("annotate"))
            else {
                continue;
            };
            // `annot.target.recognized`: only std's own `annotate` counts.
            if DefId::from_raw(value) != self.names.known.annotate {
                continue;
            }
            let mut m = 0;
            for a in deco.args.into_iter().flat_map(NodeRef::children) {
                let last = self
                    .src
                    .tokens(a)
                    .filter(|t| self.src.tkind(*t) == Some(TokenKind::Ident))
                    .last();
                if let Some(b) = last.and_then(|t| kind_bit(self.src.text(t))) {
                    m |= b;
                }
            }
            if deco.typed {
                m &= kind::FN | kind::FIELD;
            }
            mask = Some(mask.unwrap_or(0) | m);
        }
        mask
    }

    /// The fact type a decorator's value has: the result type of the
    /// function it calls, or the data type or enum it constructs.
    fn fact_type(&self, name: &str, items: &HashMap<DefId, &Item>) -> Option<DefId> {
        let (d, item) = self.scope_item(name, items)?;
        match item.data {
            ItemData::Fn(sig) => match self.names.pool.get(sig.ret) {
                TyData::Adt { def, .. } => Some(def),
                _ => None,
            },
            ItemData::Data(_) | ItemData::Enum { .. } => Some(d),
            _ => None,
        }
    }

    /// An ordinary value on a target of `kind`: a limited fact type allows
    /// only the kinds it lists (`annot.target.limit.kind-error`).
    fn target_allowed(&self, name: &str, kind: u16, items: &HashMap<DefId, &Item>) -> bool {
        let Some(fact) = self.fact_type(name, items) else {
            return true;
        };
        let mask = items
            .get(&fact)
            .map(|i| i.targets)
            .or_else(|| self.r.item(fact).map(|i| i.targets))
            .flatten();
        mask.is_none_or(|m| m & kind != 0)
    }

    fn target_error(&mut self, span: Span, what: &str) {
        self.diags.error(Code::DecoratorTargetKind, span, what);
    }

    fn marker_error(&mut self, span: Span, what: &str) {
        self.diags.error(Code::InvalidErrorMarker, span, what);
    }

    /// Whether an `@error` line's arguments are one message or
    /// `transparent` (`annot.error.form.argument`).
    fn error_args_ok(&self, args: Option<NodeRef<'_>>) -> bool {
        let Some(args) = args else { return false };
        let mut all = args.children();
        let (Some(one), None) = (all.next(), all.next()) else {
            return false;
        };
        if one.kind() != SyntaxKind::Argument {
            return false;
        }
        let mut inner = one.children();
        let (Some(value), None) = (inner.next(), inner.next()) else {
            return false;
        };
        match value.kind() {
            SyntaxKind::StringExpr => true,
            SyntaxKind::NameExpr => self
                .src
                .first_ident(value)
                .is_some_and(|t| self.src.text(t) == "transparent"),
            _ => false,
        }
    }

    /// Checks the decorators of one target; returns whether it carries a
    /// valid `@from` or `@source` cause marker.
    fn check_decorators(
        &mut self,
        n: NodeRef<'_>,
        place: Place,
        items: &HashMap<DefId, &Item>,
    ) -> bool {
        let mut cause = false;
        for d in n.children().filter(|c| c.kind() == SyntaxKind::Decorator) {
            let Some(deco) = self.deco(d) else { continue };
            let span = self.src.span(d);
            let k = place.kind;
            match deco.name.as_str() {
                "derive" => {
                    if k & (kind::FN | kind::TRAIT | kind::IMPL | kind::METHOD) != 0 {
                        let msg = "`@derive` precedes only a data type or an enum";
                        self.diags.error(Code::DecoratorNotAnnotator, span, msg);
                    }
                }
                "error" => {
                    let ok = if deco.bare {
                        k == kind::ENUM
                    } else {
                        k & (kind::DATA | kind::VARIANT) != 0
                    };
                    if !ok {
                        self.target_error(span, "this `@error` form does not precede this target");
                    } else if !deco.bare && !self.error_args_ok(deco.args) {
                        let msg = "`@error` takes one message or `transparent`";
                        self.marker_error(span, msg);
                    }
                }
                "from" | "source" if place.error_type => {
                    let ok = match k {
                        kind::FIELD => deco.name == "source" || place.members == 1,
                        _ => false,
                    };
                    if !ok {
                        let msg = format!("`@{}` does not mark this member", deco.name);
                        self.target_error(span, &msg);
                    } else if !deco.bare {
                        let msg = format!("`@{}` takes no argument", deco.name);
                        self.marker_error(span, &msg);
                    } else {
                        cause = true;
                        let bare_param = deco.name == "from"
                            && place.ty.is_some_and(|t| {
                                matches!(self.names.pool.get(t), TyData::Param(_))
                            });
                        if bare_param {
                            let msg =
                                "`@from` on a bare type parameter overlaps every other `From`";
                            self.marker_error(span, msg);
                        }
                    }
                }
                "intrinsic" => {}
                name => {
                    if !self.target_allowed(name, k, items) {
                        self.target_error(span, "the value's fact type does not allow this target");
                    }
                }
            }
        }
        cause
    }

    /// Reports a second cause member of one variant or data type
    /// (`annot.error.cause.one`).
    fn check_cause(&mut self, member: NodeRef<'_>, found: bool, causes: &mut usize) {
        if !found {
            return;
        }
        *causes += 1;
        if *causes > 1 {
            let span = self.src.span(member);
            self.marker_error(span, "a second `@from` or `@source` member");
        }
    }

    fn check_params(
        &mut self,
        list: Option<NodeRef<'_>>,
        k: u16,
        error_type: bool,
        fields: &[Field],
        items: &HashMap<DefId, &Item>,
    ) {
        let Some(list) = list else { return };
        let params: Vec<_> = list
            .children()
            .filter(|c| c.kind() == SyntaxKind::Parameter)
            .collect();
        let fields = if fields.len() == params.len() {
            fields
        } else {
            &[]
        };
        let mut causes = 0;
        for (i, p) in params.iter().enumerate() {
            let place = Place {
                kind: k,
                error_type,
                members: params.len(),
                ty: fields.get(i).map(|f| f.ty),
            };
            let found = self.check_decorators(*p, place, items);
            self.check_cause(*p, found, &mut causes);
        }
    }

    /// The item of a declaration (the name after `kw`), for its resolved
    /// member types.
    fn item_of<'i>(
        &self,
        n: NodeRef<'_>,
        kw: TokenKind,
        items: &'i HashMap<DefId, &Item>,
    ) -> Option<&'i Item> {
        let name = self.src.text(self.src.name_after(n, kw)?);
        let (d, _) = self.scope_item(name, items)?;
        items.get(&d).copied()
    }

    fn check_fn(&mut self, f: NodeRef<'_>, k: u16, items: &HashMap<DefId, &Item>) {
        self.check_decorators(
            f,
            Place {
                kind: k,
                ..Place::default()
            },
            items,
        );
        self.check_params(
            Src::child(f, SyntaxKind::ParameterList),
            kind::PARAM,
            false,
            &[],
            items,
        );
    }

    /// The members of a derivation block's `Name = [values]` line.
    fn check_member_line(
        &mut self,
        line: NodeRef<'_>,
        target: Option<(DefId, HeadKind)>,
        items: &HashMap<DefId, &Item>,
    ) {
        let Some((def, hk)) = target else { return };
        let Some(first) = self.src.tokens(line).next() else {
            return;
        };
        let k = if self.src.tkind(first) == Some(TokenKind::KwSelfType) {
            if hk == HeadKind::Enum {
                kind::ENUM
            } else {
                kind::DATA
            }
        } else if hk == HeadKind::Enum {
            let name = self.src.text(first);
            let is_variant = items.get(&def).is_some_and(|i| match &i.data {
                ItemData::Enum { variants, .. } => {
                    variants.iter().any(|v| self.names.text(v.name) == name)
                }
                _ => false,
            });
            if is_variant {
                kind::VARIANT
            } else {
                kind::FIELD
            }
        } else {
            kind::FIELD
        };
        let Some(list) = Src::child(line, SyntaxKind::ListExpr) else {
            return;
        };
        for e in list.children() {
            let callee = match e.kind() {
                SyntaxKind::CallExpr => e.children().next(),
                SyntaxKind::NameExpr => Some(e),
                _ => None,
            };
            let Some(callee) = callee.filter(|c| c.kind() == SyntaxKind::NameExpr) else {
                continue;
            };
            let Some(t) = self.src.first_ident(callee) else {
                continue;
            };
            let name = self.src.text(t).to_owned();
            if !self.target_allowed(&name, k, items) {
                let span = self.src.span(line);
                self.target_error(span, "the value's fact type does not allow this member");
            }
        }
    }

    /// Checks every decorator of the module (`annot.decorator.*`,
    /// `annot.target.*`, `annot.error.form.misplaced`).
    pub(super) fn check_module_decorators(&mut self, items: &HashMap<DefId, &Item>) {
        let src = self.src;
        for n in src.root().children() {
            let top = |k: u16| Place {
                kind: k,
                ..Place::default()
            };
            match n.kind() {
                SyntaxKind::FnDecl => self.check_fn(n, kind::FN, items),
                SyntaxKind::TypeDecl => {
                    self.check_decorators(n, top(kind::NEWTYPE), items);
                }
                SyntaxKind::DataDecl => {
                    self.check_decorators(n, top(kind::DATA), items);
                    let error_type = self.has_error(n, false);
                    let block = Src::child(n, SyntaxKind::Block);
                    let fields: Vec<_> = block
                        .into_iter()
                        .flat_map(NodeRef::children)
                        .filter(|c| {
                            matches!(c.kind(), SyntaxKind::DataField | SyntaxKind::EmbeddedField)
                        })
                        .collect();
                    let types = match self.item_of(n, TokenKind::KwData, items) {
                        Some(Item {
                            data: ItemData::Data(fs),
                            ..
                        }) if fs.len() == fields.len() => fs.as_slice(),
                        _ => &[],
                    };
                    let mut causes = 0;
                    for (i, f) in fields.iter().enumerate() {
                        let place = Place {
                            kind: kind::FIELD,
                            error_type,
                            members: fields.len(),
                            ty: types.get(i).map(|f| f.ty),
                        };
                        let found = self.check_decorators(*f, place, items);
                        self.check_cause(*f, found, &mut causes);
                    }
                }
                SyntaxKind::EnumDecl => {
                    self.check_decorators(n, top(kind::ENUM), items);
                    let error_type = self.has_error(n, true);
                    self.check_params(
                        Src::child(n, SyntaxKind::ParameterList),
                        kind::FIELD,
                        false,
                        &[],
                        items,
                    );
                    let variants = Src::child(n, SyntaxKind::Block)
                        .into_iter()
                        .flat_map(NodeRef::children)
                        .filter(|c| c.kind() == SyntaxKind::EnumVariant);
                    let declared = match self.item_of(n, TokenKind::KwEnum, items) {
                        Some(Item {
                            data: ItemData::Enum { variants, .. },
                            ..
                        }) => variants.as_slice(),
                        _ => &[],
                    };
                    for (i, v) in variants.enumerate() {
                        let place = Place {
                            kind: kind::VARIANT,
                            error_type,
                            ..Place::default()
                        };
                        self.check_decorators(v, place, items);
                        let types = declared.get(i).map_or(&[][..], |d| d.fields.as_slice());
                        self.check_params(
                            Src::child(v, SyntaxKind::ParameterList),
                            kind::FIELD,
                            error_type,
                            types,
                            items,
                        );
                    }
                }
                SyntaxKind::TraitDecl => {
                    self.check_decorators(n, top(kind::TRAIT), items);
                    let methods = Src::child(n, SyntaxKind::Block)
                        .into_iter()
                        .flat_map(NodeRef::children)
                        .filter(|c| c.kind() == SyntaxKind::FnDecl);
                    for f in methods {
                        self.check_fn(f, kind::METHOD, items);
                    }
                }
                SyntaxKind::ImplDecl => {
                    self.check_decorators(n, top(kind::IMPL), items);
                    let tys: Vec<_> = n.children().filter(|c| c.kind().is_type()).collect();
                    let target = match tys.as_slice() {
                        [t] => {
                            let segs = self.segments(*t);
                            match segs.as_slice() {
                                [(name, _)] => self
                                    .scope_item(name, items)
                                    .and_then(|(d, i)| i.kind().map(|k| (d, k))),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    let by = n.direct_tokens().any(|t| self.src.text(t) == "by");
                    for m in Src::child(n, SyntaxKind::Block)
                        .into_iter()
                        .flat_map(NodeRef::children)
                    {
                        match m.kind() {
                            SyntaxKind::FnDecl => self.check_fn(m, kind::METHOD, items),
                            SyntaxKind::Derivation if by => {
                                self.check_member_line(m, target, items);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Whether a declaration carries the `@error` line that makes it an
    /// error type: bare on an enum, with an argument on a data type.
    fn has_error(&self, n: NodeRef<'_>, bare: bool) -> bool {
        n.children()
            .filter(|c| c.kind() == SyntaxKind::Decorator)
            .filter_map(|d| self.deco(d))
            .any(|d| d.name == "error" && d.bare == bare)
    }
}
