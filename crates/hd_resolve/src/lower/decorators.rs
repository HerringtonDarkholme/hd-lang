//! Decorator target checking (annotations §Target Kinds, §Error Types).
//!
//! A header check: a decorator's fact type is the result type of the
//! function it calls, which is in the function's signature, and the type's
//! `@annotate(...)` line is declaration metadata. Neither needs a body, so
//! the check runs once the folder's items are lowered, with no checker run.

use hd_base::HashMap;

use hd_base::{DefId, PathId, Span};
use hd_diag::{Code, DiagMark};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_types::{Ty, TyData};

use super::Lower;
use crate::iface::{Field, HeadKind, ImplKind, Item, ItemData};
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
    /// The target is a derivation block (`annot.fact.unused-block-decorator`).
    block: bool,
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

    /// The declaration a decorator's possibly qualified callee reaches.
    fn decorator_def(&self, d: NodeRef<'_>) -> Option<DefId> {
        crate::facts::decorator_def(
            &self.src,
            &self.names,
            self.scope,
            &|module, name| self.r.export(module, name).ok().map(|x| x.0),
            d,
        )
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
        self.annotate_line(n).map(|(mask, _)| mask)
    }

    /// Whether the implementation `n` carries `std.structure`'s
    /// `fills_defaults()` (`annot.impl-fact.fills-defaults.decl`): only
    /// std's own function counts, including through an alias or a
    /// qualified path.
    pub(super) fn fills_defaults(&self, n: NodeRef<'_>) -> bool {
        n.children()
            .filter(|c| c.kind() == SyntaxKind::Decorator)
            .any(|d| self.decorator_def(d) == Some(self.names.known.fills_defaults))
    }

    /// The fact types that the implementation `n` names with
    /// `std.structure`'s `skips::[F]()` (`annot.impl-fact.skips.decl`),
    /// lowered among its parameters `gn`; std's own function only.
    pub(super) fn skips(&mut self, n: NodeRef<'_>, gn: &super::Gen) -> Vec<Ty> {
        let known = self.names.known.skips;
        let nodes: Vec<NodeRef<'_>> = n
            .children()
            .filter(|c| c.kind() == SyntaxKind::Decorator)
            .filter(|d| self.decorator_def(*d) == Some(known))
            .filter_map(|d| {
                let e = d.children().next()?;
                let callee = match e.kind() {
                    SyntaxKind::CallExpr => e.children().next()?,
                    _ => e,
                };
                let list = Src::child(callee, SyntaxKind::TypeArgumentList)?;
                list.children().find(|c| c.kind().is_type())
            })
            .collect();
        nodes.into_iter().map(|t| self.ty(Some(t), gn)).collect()
    }

    /// The pattern of the `@annotate::[pattern](...)` line that makes the
    /// declaration `n` a typed fact type, lowered among its parameters
    /// `gn` (`annot.typed-fact.declare`, `annot.typed-fact.pattern.scope`).
    pub(super) fn annotate_pattern(&mut self, n: NodeRef<'_>, gn: &super::Gen) -> Option<Ty> {
        if !self.annotate_line(n).is_some_and(|(_, typed)| typed) {
            return None;
        }
        let node = n
            .children()
            .filter(|c| c.kind() == SyntaxKind::Decorator)
            .filter(|d| {
                self.deco(*d)
                    .is_some_and(|x| x.typed && x.name == "annotate")
            })
            .find_map(|d| {
                let e = d.children().next()?;
                let callee = match e.kind() {
                    SyntaxKind::CallExpr => e.children().next()?,
                    _ => e,
                };
                let list = Src::child(callee, SyntaxKind::TypeArgumentList)?;
                list.children().find(|c| c.kind().is_type())
            });
        Some(match node {
            Some(t) => self.ty(Some(t), gn),
            None => Ty::POISON,
        })
    }

    fn annotate_line(&self, n: NodeRef<'_>) -> Option<(u16, bool)> {
        let mut mask = None;
        let mut typed_line = false;
        for d in n.children().filter(|c| c.kind() == SyntaxKind::Decorator) {
            let Some(deco) = self.deco(d) else { continue };
            // `annot.target.recognized`: only std's own `annotate` counts,
            // including through an alias or a qualified path.
            if self.decorator_def(d) != Some(self.names.known.annotate) {
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
                typed_line = true;
            }
            mask = Some(mask.unwrap_or(0) | m);
        }
        mask.map(|m| (m, typed_line))
    }

    /// An ordinary value on a target of `kind`: a limited fact type allows
    /// only the kinds it lists (`annot.target.limit.kind-error`).
    fn target_allowed(&self, name: &str, kind: u16, items: &HashMap<DefId, &Item>) -> bool {
        let Some((def, _)) = self.scope_item(name, items) else {
            return true;
        };
        self.target_allowed_def(def, kind, items)
    }

    /// [`Self::target_allowed`] for an already resolved decorator.
    fn target_allowed_def(&self, def: DefId, kind: u16, items: &HashMap<DefId, &Item>) -> bool {
        let Some(item) = items
            .get(&def)
            .map(|item| (*item).clone())
            .or_else(|| self.r.item(def))
        else {
            return true;
        };
        let fact = match item.data {
            ItemData::Fn(sig) => match self.names.pool.get(sig.ret) {
                TyData::Adt { def, .. } => Some(def),
                _ => None,
            },
            ItemData::Data(_) | ItemData::Enum { .. } => Some(def),
            _ => None,
        };
        let Some(fact) = fact else { return true };
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

    /// The one value in an `@error(...)` argument list, when it has one.
    fn error_arg(args: Option<NodeRef<'_>>) -> Option<NodeRef<'_>> {
        let mut all = args?.children();
        let (Some(one), None) = (all.next(), all.next()) else {
            return None;
        };
        if one.kind() != SyntaxKind::Argument {
            return None;
        }
        let mut inner = one.children();
        let (Some(value), None) = (inner.next(), inner.next()) else {
            return None;
        };
        Some(value)
    }

    /// Whether an `@error` line's arguments are one message or
    /// `transparent` (`annot.error.form.argument`).
    fn error_args_ok(&self, args: Option<NodeRef<'_>>) -> bool {
        Self::error_arg(args).is_some_and(|value| match value.kind() {
            // A prefixed string runs a user function, so it is no message
            // (`annot.error.message.string`).
            SyntaxKind::StringExpr => self.src.text(self.src.first(value)).starts_with('"'),
            SyntaxKind::NameExpr => self
                .src
                .first_ident(value)
                .is_some_and(|t| self.src.text(t) == "transparent"),
            _ => false,
        })
    }

    /// Whether an `@error(...)` line names the `transparent` form.
    fn error_transparent(&self, args: Option<NodeRef<'_>>) -> bool {
        Self::error_arg(args).is_some_and(|value| {
            value.kind() == SyntaxKind::NameExpr
                && self
                    .src
                    .first_ident(value)
                    .is_some_and(|t| self.src.text(t) == "transparent")
        })
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
            let span = self.src.span(d);
            let mark = self.diags.mark();
            let Some(deco) = self.deco(d) else {
                self.record_block_decorator(place, None, n, d, mark);
                continue;
            };
            let k = place.kind;
            match deco.name.as_str() {
                "host" => {
                    if k != kind::TRAIT || !self.src.is_pub(n) {
                        self.target_error(
                            span,
                            "`@host` precedes only a package-owned `pub trait`",
                        );
                    } else if !deco.bare || deco.typed {
                        self.target_error(
                            span,
                            "write bare `@host`, without arguments or type arguments",
                        );
                    }
                }
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
                    } else if !deco.bare && self.error_transparent(deco.args) && place.members != 1
                    {
                        self.target_error(span, "`@error(transparent)` needs exactly one member");
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
                "intrinsic" => {
                    // `trait.impl.intrinsic.std-only`: only the standard
                    // library sees the name `intrinsic`.
                    if !self.in_std() {
                        self.diags.error(
                            Code::UnknownName,
                            span,
                            "`intrinsic` names no binding outside the standard library",
                        );
                    }
                }
                "from" | "source" => {
                    // Outside an error type each is an ordinary decorator
                    // (`annot.error.marker.outside`): it must resolve.
                    if let Some(def) = self.decorator_def(d) {
                        if !self.target_allowed_def(def, k, items) {
                            self.target_error(
                                span,
                                "the value's fact type does not allow this target",
                            );
                        }
                    } else {
                        self.diags.error(
                            Code::UnknownName,
                            span,
                            &format!("`@{}` names no binding", deco.name),
                        );
                    }
                }
                _ => {
                    if self
                        .decorator_def(d)
                        .is_some_and(|def| !self.target_allowed_def(def, k, items))
                    {
                        self.target_error(span, "the value's fact type does not allow this target");
                    }
                }
            }
            self.record_block_decorator(place, Some(deco.name.as_str()), n, d, mark);
        }
        cause
    }

    /// Records the resolved occurrence; tooling decides whether to advise on it.
    fn record_block_decorator(
        &mut self,
        place: Place,
        name: Option<&str>,
        target: NodeRef<'_>,
        decorator: NodeRef<'_>,
        mark: DiagMark,
    ) {
        if !place.block {
            return;
        }
        let Some(facts) = self.derivation_facts.as_deref_mut() else {
            return;
        };
        facts.decorators.push(crate::facts::BlockDecorator {
            span: self.src.span(decorator),
            value: decorator.children().next().map(|n| self.src.span(n)),
            first: Src::child(target, SyntaxKind::Block)
                .and_then(|b| b.children().next())
                .map(|n| self.src.span(n)),
            marker: matches!(name, Some("intrinsic" | "from" | "source")),
            valid: self.diags.errors_since(mark) == 0,
        });
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
                ..Place::default()
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

    /// The `Item::inline` value of one decorator line that names std's
    /// `inline` or `cold` (`annot.inline.recognized`), and whether its
    /// argument is one the compiler reads (`annot.inline.argument`);
    /// `None` for any other line.
    fn inline_value(&self, d: NodeRef<'_>) -> Option<(u8, bool)> {
        use crate::iface::{COLD, INLINE_ALWAYS, INLINE_AUTO, INLINE_NEVER};
        let def = self.decorator_def(d)?;
        let cold = def == self.names.known.cold;
        if !cold && def != self.names.known.inline {
            return None;
        }
        let e = d.children().next()?;
        let (callee, args) = match e.kind() {
            SyntaxKind::CallExpr => (
                e.children().next()?,
                Src::child(e, SyntaxKind::ArgumentList),
            ),
            _ => (e, None),
        };
        let typed = callee.kind() == SyntaxKind::TypeArgsExpr;
        let args: Vec<NodeRef<'_>> = args.into_iter().flat_map(NodeRef::children).collect();
        if cold {
            return Some((COLD, !typed && args.is_empty()));
        }
        let hint = match args.as_slice() {
            [] => Some(INLINE_AUTO),
            [a] => {
                let named = a.kind() == SyntaxKind::NamedArgument;
                let value = a.children().next().filter(|v| {
                    v.kind() == SyntaxKind::VariantExpr
                        && (a.kind() == SyntaxKind::Argument
                            || named && self.src.text(self.src.first(*a)) == "hint")
                });
                let last = value.and_then(|v| {
                    self.src
                        .tokens(v)
                        .filter(|t| self.src.tkind(*t) == Some(TokenKind::Ident))
                        .last()
                });
                last.and_then(|t| match self.src.text(t) {
                    "Auto" => Some(INLINE_AUTO),
                    "Always" => Some(INLINE_ALWAYS),
                    "Never" => Some(INLINE_NEVER),
                    _ => None,
                })
            }
            _ => None,
        };
        Some((hint.unwrap_or(INLINE_AUTO), !typed && hint.is_some()))
    }

    /// A function's or method's `@inline` or `@cold` choice, as
    /// `Item::inline` holds it, or 0 (`annot.inline.recognized`).
    pub(super) fn inline_hint(&self, n: NodeRef<'_>) -> u8 {
        n.children()
            .filter(|c| c.kind() == SyntaxKind::Decorator)
            .find_map(|d| self.inline_value(d))
            .map_or(0, |(v, _)| v)
    }

    /// Checks the `@inline` and `@cold` lines of a function or method
    /// (`annot.inline.*`): a valid argument, a body of its own, and one
    /// choice.
    fn check_inline(&mut self, f: NodeRef<'_>) {
        use crate::iface::COLD;
        let bodyless = Src::child(f, SyntaxKind::Block).is_none();
        let intrinsic = self.intrinsic(f).is_some();
        let mut first: Option<u8> = None;
        for d in f.children().filter(|c| c.kind() == SyntaxKind::Decorator) {
            let Some((v, ok)) = self.inline_value(d) else {
                continue;
            };
            let span = self.src.span(d);
            let code = Code::InvalidInlineDecorator;
            if !ok {
                let msg = if v == COLD {
                    "`@cold` takes no argument"
                } else {
                    "`@inline` takes `.Auto`, `.Always` or `.Never`"
                };
                self.diags.error(code, span, msg);
            } else if bodyless || intrinsic {
                let msg = "only a function or method with a body of its own takes this decorator";
                self.diags.error(code, span, msg);
            } else if let Some(p) = first {
                if (p == COLD) == (v == COLD) {
                    let msg = "a second inlining choice for this function";
                    self.diags.error(Code::DuplicateFact, span, msg);
                } else {
                    let msg = "`@cold` already keeps this function from being inlined; write one of the two";
                    self.diags.error(code, span, msg);
                }
            } else {
                first = Some(v);
            }
        }
    }

    fn check_fn(&mut self, f: NodeRef<'_>, k: u16, items: &HashMap<DefId, &Item>) {
        self.check_inline(f);
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

    /// Whether a member line's right side is the omit form `pass`.
    fn is_omit(&self, rhs: NodeRef<'_>) -> bool {
        rhs.kind() == SyntaxKind::LiteralExpr
            && self
                .src
                .tokens(rhs)
                .any(|t| self.src.tkind(t) == Some(TokenKind::KwPass))
    }

    /// The package node declaring `d`, without rendering its module.
    fn package_of(&self, d: DefId) -> PathId {
        self.names.package_node(d)
    }

    /// One member line's drift signature (`annot.line.drift`): the member
    /// it names, whether it extends (`+=`), and the right side's tokens
    /// (`pass` for an omit).
    fn drift_lines(&self, n: NodeRef<'_>) -> Vec<(String, bool, Vec<String>)> {
        let mut out = Vec::new();
        let Some(body) = Src::child(n, SyntaxKind::Block) else {
            return out;
        };
        for line in body
            .children()
            .filter(|c| c.kind() == SyntaxKind::Derivation)
        {
            let mut toks = self.src.tokens(line);
            let (Some(name), Some(op)) = (toks.next(), toks.next()) else {
                continue;
            };
            if !matches!(
                self.src.tkind(name),
                Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfType)
            ) {
                continue;
            }
            let Some(rhs) = line.children().last() else {
                continue;
            };
            let rhs = if self.is_omit(rhs) {
                vec!["pass".to_owned()]
            } else {
                self.src
                    .tokens(rhs)
                    .map(|t| self.src.text(t).to_owned())
                    .collect()
            };
            out.push((
                self.src.text(name).to_owned(),
                self.src.tkind(op) == Some(TokenKind::PlusEq),
                rhs,
            ));
        }
        out
    }

    /// A `by` derivation block for the drift check: its target, its
    /// trait's package, the trait and target names as written, its member
    /// lines, and the block's span.
    fn collect_drift_block(
        &self,
        n: NodeRef<'_>,
        tys: &[NodeRef<'_>],
        target: Option<(DefId, HeadKind)>,
        items: &HashMap<DefId, &Item>,
        out: &mut Vec<crate::facts::DerivationBlock>,
    ) {
        let Some((target_def, hk)) = target else {
            return;
        };
        if !matches!(hk, HeadKind::Data | HeadKind::Enum) {
            return;
        }
        let [trait_ty, target_ty] = tys else {
            return;
        };
        let trait_segs = self.segments(*trait_ty);
        let [(trait_name, _)] = trait_segs.as_slice() else {
            return;
        };
        let target_segs = self.segments(*target_ty);
        let [(target_name, _)] = target_segs.as_slice() else {
            return;
        };
        let Some((trait_def, item)) = self.scope_item(trait_name, items) else {
            return;
        };
        if !matches!(item.data, ItemData::Trait(_)) {
            return;
        }
        out.push(crate::facts::DerivationBlock {
            folder: self.r.cx.folder,
            target: target_def,
            package: self.package_of(trait_def),
            trait_: trait_def,
            trait_name: trait_name.clone(),
            target_name: target_name.clone(),
            lines: self.drift_lines(n),
            span: self.src.span(n),
        });
    }

    /// The form of a member line (`annot.line.right.error`,
    /// `annot.line.right.not-list`, `annot.no-trait.no-omit`): `pass` omits
    /// a member by `=` alone, and not in a block with no trait; `Self` and
    /// a whole variant cannot be omitted; any other right side is a list.
    /// A right side whose form can never have a list type is rejected here;
    /// one that might (a name, a call) is typed with the module's bodies
    /// (`hd_check::check_decl_facts`).
    fn check_line_form(&mut self, line: NodeRef<'_>, variant: bool, traitless: bool) {
        let Some(rhs) = line.children().last() else {
            return;
        };
        let toks = &self.src.parse.tokens;
        let on_self = self
            .src
            .tokens(line)
            .next()
            .is_some_and(|t| self.src.tkind(t) == Some(TokenKind::KwSelfType));
        let why = if self.is_omit(rhs) {
            if line.direct_token(toks, TokenKind::PlusEq).is_some() {
                "`+=` extends a list and cannot take `pass`"
            } else if on_self {
                "`Self` cannot be omitted"
            } else if variant {
                "a whole variant cannot be omitted"
            } else if traitless {
                "a block with no trait writes only metadata, so it cannot omit a member"
            } else {
                return;
            }
        } else if crate::facts::never_a_list(rhs.kind()) {
            "the right side of a member line is a list"
        } else {
            return;
        };
        self.diags
            .error(Code::InvalidMemberLine, self.src.span(line), why);
    }

    /// Whether a member line's name is a direct member of its target (a
    /// whole variant for enums), or `Self` (`annot.line.name`,
    /// `annot.line.enum`). Unresolvable targets and non-name lines are
    /// left to other checks.
    fn line_names_member(
        &self,
        first: hd_base::TokenIdx,
        target: Option<(DefId, HeadKind)>,
        items: &HashMap<DefId, &Item>,
    ) -> bool {
        if self.src.tkind(first) == Some(TokenKind::KwSelfType) {
            return true;
        }
        if !matches!(
            self.src.tkind(first),
            Some(TokenKind::Ident | TokenKind::RawIdent)
        ) {
            return true;
        }
        let Some((def, _)) = target else {
            return true;
        };
        let Some(item) = items.get(&def) else {
            return true;
        };
        let name = self.src.text(first);
        match &item.data {
            ItemData::Data(fields) => fields.iter().any(|f| {
                let n = self.names.text(f.name);
                n == name || name.strip_prefix('_') == Some(n)
            }),
            ItemData::Enum { variants, .. } => {
                variants.iter().any(|v| self.names.text(v.name) == name)
            }
            _ => true,
        }
    }

    /// The members of a derivation block's `Name = [values]` line.
    fn check_member_line(
        &mut self,
        line: NodeRef<'_>,
        target: Option<(DefId, HeadKind)>,
        traitless: bool,
        items: &HashMap<DefId, &Item>,
    ) {
        let Some((def, hk)) = target else { return };
        let Some(first) = self.src.tokens(line).next() else {
            return;
        };
        let is_variant = hk == HeadKind::Enum
            && self.src.tkind(first) != Some(TokenKind::KwSelfType)
            && items.get(&def).is_some_and(|i| match &i.data {
                ItemData::Enum { variants, .. } => variants
                    .iter()
                    .any(|v| self.names.text(v.name) == self.src.text(first)),
                _ => false,
            });
        self.check_line_form(line, is_variant, traitless);
        // `annot.line.name`, `annot.line.enum`: the name is a direct
        // member of the target (a whole variant for enums), or `Self`.
        if !self.line_names_member(first, target, items) {
            let name = self.src.text(first).to_owned();
            let target_name = target.and_then(|(d, _)| items.get(&d)).map_or_else(
                || "the target".to_owned(),
                |i| self.names.text(i.name).to_owned(),
            );
            let msg = if target.is_some_and(|(_, hk)| hk == HeadKind::Enum) {
                format!("`{name}` is not a variant of `{target_name}`")
            } else {
                format!("`{name}` is not a member of `{target_name}`")
            };
            self.diags
                .error(Code::UnknownAnnotationMember, self.src.span(line), &msg);
        }
        // Fact targets are checked in a block with no trait.
        if !traitless {
            return;
        }
        let k = if self.src.tkind(first) == Some(TokenKind::KwSelfType) {
            if hk == HeadKind::Enum {
                kind::ENUM
            } else {
                kind::DATA
            }
        } else if is_variant {
            kind::VARIANT
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
    pub(super) fn check_module_decorators(
        &mut self,
        items: &HashMap<DefId, &Item>,
        blocks: &[hd_base::NodeIdx],
    ) {
        let src = self.src;
        let mut drift_blocks = Vec::new();
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
                    let error_type = self.has_error(n, false);
                    let block = Src::child(n, SyntaxKind::Block);
                    let fields: Vec<_> = block
                        .into_iter()
                        .flat_map(NodeRef::children)
                        .filter(|c| {
                            matches!(c.kind(), SyntaxKind::DataField | SyntaxKind::EmbeddedField)
                        })
                        .collect();
                    self.check_decorators(
                        n,
                        Place {
                            kind: kind::DATA,
                            members: fields.len(),
                            ..Place::default()
                        },
                        items,
                    );
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
                            ..Place::default()
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
                        .filter(|c| c.kind() == SyntaxKind::EnumVariant)
                        .collect::<Vec<_>>();
                    self.check_default_variant(n, &variants);
                    let declared = match self.item_of(n, TokenKind::KwEnum, items) {
                        Some(Item {
                            data: ItemData::Enum { variants, .. },
                            ..
                        }) => variants.as_slice(),
                        _ => &[],
                    };
                    for (i, v) in variants.into_iter().enumerate() {
                        let members = Src::child(v, SyntaxKind::ParameterList)
                            .into_iter()
                            .flat_map(NodeRef::children)
                            .filter(|c| c.kind() == SyntaxKind::Parameter)
                            .count();
                        let place = Place {
                            kind: kind::VARIANT,
                            error_type,
                            members,
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
                    let place = Place {
                        kind: kind::IMPL,
                        block: blocks.contains(&n.index()),
                        ..Place::default()
                    };
                    self.check_decorators(n, place, items);
                    let tys: Vec<_> = n.children().filter(|c| c.kind().is_type()).collect();
                    let target = tys.last().and_then(|t| {
                        let segs = self.segments(*t);
                        match segs.as_slice() {
                            [(name, _)] => self
                                .scope_item(name, items)
                                .and_then(|(d, i)| i.kind().map(|k| (d, k))),
                            _ => None,
                        }
                    });
                    let by = n.direct_tokens().any(|t| self.src.text(t) == "by");
                    let traitless = tys.len() == 1;
                    for m in Src::child(n, SyntaxKind::Block)
                        .into_iter()
                        .flat_map(NodeRef::children)
                    {
                        match m.kind() {
                            SyntaxKind::FnDecl => self.check_fn(m, kind::METHOD, items),
                            SyntaxKind::Derivation if by => {
                                self.check_member_line(m, target, traitless, items);
                            }
                            _ => {}
                        }
                    }
                    if by && !traitless && self.derivation_facts.is_some() {
                        self.collect_drift_block(n, &tys, target, items, &mut drift_blocks);
                    }
                }
                _ => {}
            }
        }
        let derived = if self.derivation_facts.is_some() {
            items
                .values()
                .filter_map(|it| match &it.data {
                    ItemData::Impl {
                        trait_,
                        self_ty,
                        kind: ImplKind::Derived,
                        ..
                    } => {
                        let TyData::Adt { def, .. } = self.names.pool.get(*self_ty) else {
                            return None;
                        };
                        Some(crate::facts::Derived {
                            folder: self.r.cx.folder,
                            target: def,
                            package: self.package_of(*trait_),
                            trait_: *trait_,
                            name: self.names.display_name(*trait_).to_owned(),
                        })
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if let Some(facts) = self.derivation_facts.as_deref_mut() {
            facts.blocks.extend(drift_blocks);
            facts.derived.extend(derived);
        }
    }

    /// `std-ops.default.derive.one-variant`: an enum that derives `Default`
    /// marks exactly one variant `@default`. None is an error on the
    /// `@derive` line, several on the second marker.
    fn check_default_variant(&mut self, n: NodeRef<'_>, variants: &[NodeRef<'_>]) {
        let known = self.names.known;
        let is = |name: &str, def: DefId| {
            matches!(
                self.scope.lookup(self.sym(name)),
                Some(Binding { kind: BindingKind::Item, value }) if DefId::from_raw(value) == def
            )
        };
        let derive = n
            .children()
            .filter(|c| c.kind() == SyntaxKind::Decorator)
            .find(|d| {
                self.deco(*d).is_some_and(|deco| {
                    deco.name == "derive"
                        && deco.args.into_iter().flat_map(NodeRef::children).any(|a| {
                            a.descendants()
                                .filter(|x| x.kind() == SyntaxKind::NameExpr)
                                .filter_map(|x| self.src.first_ident(x))
                                .any(|t| is(self.src.text(t), known.default))
                        })
                })
            });
        let Some(derive) = derive else { return };
        let marker = self.names.item("std.ops", "default");
        let marks: Vec<NodeRef<'_>> = variants
            .iter()
            .flat_map(|v| v.children().filter(|c| c.kind() == SyntaxKind::Decorator))
            .filter(|d| {
                self.deco(*d)
                    .is_some_and(|deco| deco.bare && self.decorator_def(*d) == Some(marker))
            })
            .collect();
        match marks.as_slice() {
            [_] => {}
            [] => {
                let msg =
                    "a derived `Default` enum marks one variant `@default`, and none is marked";
                self.diags
                    .error(Code::InvalidDefaultVariant, self.src.span(derive), msg);
            }
            [_, second, ..] => {
                let msg =
                    "a derived `Default` enum marks one variant `@default`, and a second is marked";
                self.diags
                    .error(Code::InvalidDefaultVariant, self.src.span(*second), msg);
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
