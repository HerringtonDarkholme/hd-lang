//! Local references (spec/lang/13-values-and-references.md, "Local
//! References"): `&T` and `&mut T` are lent to a call and never kept, so
//! a local reference that reaches a field, a collection element, a
//! top-level binding, a function type's result, a closure capture, a
//! type argument or a counted reference is `local-reference-escapes`,
//! whose message names that position and offers no fix-it. A function's
//! result may hold one, whose source its signature elides
//! ([`declarations`], `crate::dep_results`).
//!
//! Both kinds have one representation (`ref.type.kind-runtime`): they
//! lower to `Ref[T]` and `Counted[T]` (`hd_types::Types::lower_refs`),
//! which differ only in what the backend knows of the address, so
//! nothing here changes the body's code. Three checks cover the positions:
//! [`declarations`] reads the written types of a module, `coerce` reports
//! a local reference where a counted one is expected
//! ([`Ck::coerce_ref_kind`]), and the body's end reports the inferred
//! types that hold one ([`Ck::check_local_ref_types`]). Captures report
//! when their closure closes ([`Ck::local_ref_captures`]), except for a
//! closure passed directly to a `fn(...)` parameter; dependent-result
//! solving tracks that closure's capture origins.

use hd_base::{HashSet, LocalId, TokenIdx};
use hd_diag::{Code, DiagBuf};
use hd_resolve::view::Src;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::Ref;
use hd_types::{Ty, TyData, Types};

use crate::body::{Ck, ExpectPosition};

/// Where a local reference escapes to (`ref.local.escape.*`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Escape {
    Field,
    Element,
    TopLevel,
    Result,
    Capture,
    TypeArgument,
    /// A position that expects a counted reference
    /// (`ref.local.escape.counted`).
    Counted,
    /// A `*self` call through a local reference
    /// (`fn.method.counted-self.local-receiver`).
    CountedSelf,
}

impl Escape {
    /// The position as the message names it
    /// (`ref.local.escape.position-message`).
    fn what(self) -> &'static str {
        match self {
            Escape::Field => "a field",
            Escape::Element => "a collection element",
            Escape::TopLevel => "a top-level binding",
            Escape::Result => "a function value's result, which has no source for it",
            Escape::Capture => "a closure capture",
            Escape::TypeArgument => "a type argument",
            Escape::Counted => "a counted reference",
            Escape::CountedSelf => "a counted reference, as this `*self` call takes one",
        }
    }

    /// The position of a value that `coerce` converts for `what`.
    fn of_value(what: &str) -> Escape {
        match what {
            "field" => Escape::Field,
            "list element" | "map key" | "map value" | "element" => Escape::Element,
            "return" | "result" => Escape::Result,
            _ => Escape::Counted,
        }
    }
}

fn message(at: Escape) -> String {
    format!("a local reference never escapes to {}", at.what())
}

/// The message for a local reference that no written `&` declares, as
/// `&e` makes (`ref.expr.kind.no-counted.advice`).
fn box_message(at: Escape) -> String {
    format!(
        "a local reference never escapes to {}; box the value at its declaration, as `x := box(value)`",
        at.what()
    )
}

/// The value a field initializer or an argument passes, without its
/// parentheses.
fn value_of(e: NodeRef<'_>) -> NodeRef<'_> {
    let e = crate::elem_refs::strip_parens(e);
    match e.kind() {
        SyntaxKind::DataFieldInit | SyntaxKind::Argument | SyntaxKind::NamedArgument => e
            .children()
            .last()
            .map_or(e, crate::elem_refs::strip_parens),
        _ => e,
    }
}

/// A written local reference type in a position that keeps it
/// (`ref.local.escape.field`, `.top-level`, `.generic-argument`): one
/// error per `&`. A parameter, a local binding, an impl's target, a
/// function's result and a function type's parameters and result may be
/// local references. A function type's result that holds one needs one
/// source (`fn.type.lifetime.elide`); a function's is the resolver's
/// (`ref.dep.elide`).
pub fn declarations(src: &Src<'_>, diags: &mut DiagBuf) {
    for item in src.root().children() {
        match item.kind() {
            // A top-level binding's annotation (`ref.local.escape.top-level`).
            SyntaxKind::LetStmt => {
                for c in item.children() {
                    if c.kind().is_type() {
                        walk_ty(src, c, Some(Escape::TopLevel), true, diags);
                    } else {
                        walk(src, c, false, diags);
                    }
                }
            }
            _ => walk(src, item, false, diags),
        }
    }
}

/// Every type under `n` outside a type, in its declaration's position.
fn walk(src: &Src<'_>, n: NodeRef<'_>, body: bool, diags: &mut DiagBuf) {
    match n.kind() {
        SyntaxKind::DataField => {
            for c in n.children() {
                if c.kind().is_type() {
                    walk_ty(src, c, Some(Escape::Field), true, diags);
                } else {
                    walk(src, c, body, diags);
                }
            }
        }
        // An enum variant's payload fields.
        SyntaxKind::EnumVariant => {
            for c in n.children() {
                if c.kind() == SyntaxKind::ParameterList {
                    for p in c.children() {
                        for t in p.children() {
                            if t.kind().is_type() {
                                walk_ty(src, t, Some(Escape::Field), true, diags);
                            } else {
                                walk(src, t, body, diags);
                            }
                        }
                    }
                } else {
                    walk(src, c, body, diags);
                }
            }
        }
        // A function's written result's source is the resolver's
        // (`ref.dep.elide`); a local function's is a closure's.
        SyntaxKind::FnDecl => {
            for c in n.children() {
                if c.kind() == SyntaxKind::Block {
                    local_fns(src, c, diags);
                }
                walk(src, c, c.kind() == SyntaxKind::Block, diags);
            }
        }
        // A test case's suite is a body.
        SyntaxKind::TrailingCallExpr => {
            for c in n.children() {
                walk(src, c, true, diags);
            }
        }
        // A closure's written result may hold a local reference, whose
        // source the checker finds from its parameters' types
        // (`fn.closure.lifetime`).
        SyntaxKind::ClosureExpr => {
            for c in n.children() {
                if c.kind().is_type() {
                    walk_ty(src, c, None, !body, diags);
                } else {
                    walk(src, c, body, diags);
                }
            }
        }
        // An alias holds what its type holds (`ref.local.holding`), so its
        // uses report. A newtype's value is its one field.
        SyntaxKind::TypeDecl => {
            if n.direct_token(&src.parse.tokens, TokenKind::Eq).is_none() {
                for c in n.children().filter(|c| c.kind().is_type()) {
                    walk_ty(src, c, Some(Escape::Field), true, diags);
                }
            }
        }
        // An expression's explicit type arguments, as in `f::[&T]()`, may
        // hold a local reference (`ref.generic.local`).
        SyntaxKind::TypeArgumentList => {
            for c in n.children() {
                walk_ty(src, c, None, !body, diags);
            }
        }
        // `ref.lifetime.store.type`: a `&mut` parameter's list or
        // map may hold local references of written lifetimes.
        SyntaxKind::Parameter if !body => {
            for c in n.children() {
                if c.kind().is_type() {
                    walk_ty(src, c, None, !lifetime_place(src, c), diags);
                } else {
                    walk(src, c, body, diags);
                }
            }
        }
        _ if n.kind().is_type() => walk_ty(src, n, None, !body, diags),
        _ => {
            for c in n.children() {
                walk(src, c, body, diags);
            }
        }
    }
}

/// The type `n` in position `at`: `None` where a local reference may
/// stand, as a parameter's type does. `keep` is off in a body and for a
/// `&mut` parameter of written lifetimes, where a list's or a map's type
/// argument may hold one (`ref.lifetime.store.type`).
fn walk_ty(src: &Src<'_>, n: NodeRef<'_>, at: Option<Escape>, keep: bool, diags: &mut DiagBuf) {
    match n.kind() {
        SyntaxKind::RefType => {
            let amp = n.direct_token(&src.parse.tokens, TokenKind::Amp);
            // `ref.lifetime.struct.field`: a field's `&'a T` names a
            // lifetime of its declaration, which the resolver checks.
            let named = at == Some(Escape::Field)
                && n.direct_token(&src.parse.tokens, TokenKind::Lifetime)
                    .is_some();
            if let (Some(_), Some(at), false) = (amp, at, named) {
                diags.error(Code::LocalReferenceEscapes, src.span(n), &message(at));
            }
            // A reference may refer to a local reference (`ref.type.group`).
            for c in n.children() {
                walk_ty(src, c, None, keep, diags);
            }
        }
        // `ref.local.holding`: a tuple holds what its elements hold.
        SyntaxKind::TupleType | SyntaxKind::ParenType => {
            for c in n.children() {
                walk_ty(src, c, at, keep, diags);
            }
        }
        // `ref.local.holding`: an optional, and a generic type's instance,
        // hold what their type arguments hold, where a local reference may
        // stand (`ref.dep.result`). A list's or a map's type arguments
        // hold one only where `keep` is off (`ref.local.escape.type-argument`).
        SyntaxKind::OptionalType => {
            for c in n.children() {
                walk_ty(src, c, at, keep, diags);
            }
        }
        SyntaxKind::NamedType => {
            let toks = &src.parse.tokens;
            let name = n
                .direct_tokens()
                .filter(|&t| matches!(toks.kind(t), TokenKind::Ident | TokenKind::RawIdent))
                .last()
                .map(|t| src.text(t));
            let at = if keep && matches!(name, Some("List" | "Map")) {
                Some(Escape::TypeArgument)
            } else {
                at
            };
            for c in n.children() {
                if c.kind() == SyntaxKind::TypeArgumentList {
                    for a in c.children() {
                        walk_ty(src, a, at, keep, diags);
                    }
                } else {
                    walk_ty(src, c, None, keep, diags);
                }
            }
        }
        // `ref.local.escape.type-argument`: an expression's or a
        // trait's type arguments.
        SyntaxKind::TypeArgumentList => {
            for c in n.children() {
                walk_ty(src, c, Some(Escape::TypeArgument), keep, diags);
            }
        }
        // A function type's parameters may be local, and so may its
        // result, which needs a source as a function's does
        // (`fn.type.lifetime.elide`).
        SyntaxKind::FunctionType => {
            let arrow = n
                .direct_token(&src.parse.tokens, TokenKind::Arrow)
                .map(|t| src.token_span(t).lo);
            let mut ins = Vec::new();
            let mut outs = Vec::new();
            let mut ret = None;
            for c in n.children() {
                let result = arrow.is_some_and(|a| src.span(c).lo > a);
                if c.kind().is_type() && c.kind() != SyntaxKind::RequirementRow {
                    if result {
                        ref_positions(src, c, &mut outs);
                        ret = Some(c);
                    } else {
                        ref_positions(src, c, &mut ins);
                    }
                }
                walk_ty(src, c, None, keep, diags);
            }
            if let Some(ret) = ret
                && outs.iter().any(Option::is_none)
            {
                fn_type_source(src, ret, &ins, diags);
            }
        }
        _ => {
            for c in n.children() {
                if c.kind().is_type() || c.kind() == SyntaxKind::TypeArgumentList {
                    walk_ty(src, c, None, keep, diags);
                } else {
                    walk(src, c, !keep, diags);
                }
            }
        }
    }
}

/// Whether the written parameter type `n` is a `&mut` whose every other
/// `&` writes a lifetime (`ref.lifetime.store.type`).
fn lifetime_place(src: &Src<'_>, n: NodeRef<'_>) -> bool {
    let toks = &src.parse.tokens;
    let amp = |r: NodeRef<'_>| {
        r.kind() == SyntaxKind::RefType && r.direct_token(toks, TokenKind::Amp).is_some()
    };
    amp(n)
        && n.direct_token(toks, TokenKind::KwMut).is_some()
        && n.descendants()
            .filter(|&d| d.index() != n.index() && amp(d))
            .all(|d| d.direct_token(toks, TokenKind::Lifetime).is_some())
}

/// The local reference positions of the written type `n`
/// (`ref.dep.position`): each `&` outside a function type, with the
/// lifetime it writes.
fn ref_positions<'s>(src: &Src<'s>, n: NodeRef<'_>, out: &mut Vec<Option<&'s str>>) {
    if n.kind() == SyntaxKind::FunctionType {
        return;
    }
    let toks = &src.parse.tokens;
    if n.kind() == SyntaxKind::RefType && n.direct_token(toks, TokenKind::Amp).is_some() {
        out.push(
            n.direct_token(toks, TokenKind::Lifetime)
                .map(|t| src.text(t)),
        );
    }
    for c in n.children() {
        ref_positions(src, c, out);
    }
}

/// Each local function in the body `n`, outside a nested function or a
/// local implementation or trait: a closure, so a written result that
/// holds a local reference needs one source as a function type's does
/// (`fn.closure.lifetime`).
fn local_fns(src: &Src<'_>, n: NodeRef<'_>, diags: &mut DiagBuf) {
    for c in n.children() {
        match c.kind() {
            SyntaxKind::FnDecl => {
                let Some(ret) = c
                    .children()
                    .find(|t| t.kind().is_type() && t.kind() != SyntaxKind::RequirementRow)
                else {
                    continue;
                };
                let mut outs = Vec::new();
                ref_positions(src, ret, &mut outs);
                if outs.iter().any(Option::is_none) {
                    let mut ins = Vec::new();
                    for p in c
                        .children()
                        .filter(|p| p.kind() == SyntaxKind::ParameterList)
                    {
                        ref_positions(src, p, &mut ins);
                    }
                    fn_type_source(src, ret, &ins, diags);
                }
            }
            SyntaxKind::ImplDecl | SyntaxKind::TraitDecl => {}
            _ => local_fns(src, c, diags),
        }
    }
}

/// `fn.type.lifetime.elide`: a result position of a function type that
/// writes no lifetime takes the inputs' one lifetime, `ins` holding each
/// input position's. None is `reference-result-without-source`, and two
/// or more are `ambiguous-reference-source`.
fn fn_type_source(src: &Src<'_>, ret: NodeRef<'_>, ins: &[Option<&str>], diags: &mut DiagBuf) {
    if ins.is_empty() {
        diags.error(
            Code::ReferenceResultWithoutSource,
            src.span(ret),
            "a reference result needs a reference parameter to borrow from",
        );
        return;
    }
    let first = ins[0];
    if first.is_none() && ins.len() > 1 || ins.iter().any(|l| *l != first) {
        diags.error(
            Code::AmbiguousReferenceSource,
            src.span(ret),
            "the result may borrow from more than one reference parameter; write a lifetime to name its source",
        );
    }
}

/// Where a value of type `t` keeps a local reference: in a type argument
/// of a trait value or an opaque type, or as a
/// function type's result. A local reference at the top (`top_ok`), in a
/// tuple, an optional or another generic type's instance at the top
/// (`ref.local.holding`), as a function type's parameter or behind a
/// reference is no escape.
fn escape_in(pool: Types<'_>, t: Ty, top_ok: bool, depth: u32) -> Option<Escape> {
    if depth > 32 || !pool.has_ref(t) {
        return None;
    }
    let d = depth + 1;
    let escape_in = |t, top| escape_in(pool, t, top, d);
    let list = |l, top: bool| pool.list_items(l).iter().find_map(|&a| escape_in(a, top));
    match pool.get(t) {
        TyData::Ref {
            counted: false,
            inner,
            ..
        } => {
            if top_ok {
                escape_in(inner, true)
            } else {
                Some(Escape::TypeArgument)
            }
        }
        TyData::Ref { inner, .. } => escape_in(inner, true),
        TyData::Mut(i) | TyData::Option(i) => escape_in(i, top_ok),
        TyData::Tuple { elems, rest } => {
            list(elems, top_ok).or_else(|| rest.and_then(|r| escape_in(r, top_ok)))
        }
        TyData::Adt { args, .. } => list(args, top_ok),
        // An opaque type that borrows holds a local reference of its own,
        // at the top as a reference is (`types.opaque.borrows`).
        TyData::Opaque { borrows: true, .. } if !top_ok => Some(Escape::TypeArgument),
        TyData::Opaque { args, .. } => list(args, false),
        TyData::TraitValue { args, bindings, .. } => {
            list(args, false).or_else(|| bindings.iter().find_map(|&(_, b)| escape_in(b, false)))
        }
        TyData::Assoc { self_ty, args, .. } => {
            escape_in(self_ty, false).or_else(|| list(args, false))
        }
        // A result with a source may hold a local reference at the top,
        // as a function's may (`fn.type.lifetime.result`).
        TyData::Fn { params, result, .. } => list(params, true).or_else(|| {
            let sourced = pool.fn_ty_deps(t).is_some();
            match escape_in(result, sourced) {
                Some(Escape::TypeArgument) if !sourced && is_local_ref(pool, result) => {
                    Some(Escape::Result)
                }
                other => other,
            }
        }),
        _ => None,
    }
}

/// Whether a value of type `t` holds a local reference anywhere, its top
/// included (`ref.local.holding`), as `box` checks (`ref.box.no-local`).
pub(crate) fn holds_local_ref(pool: Types<'_>, t: Ty) -> bool {
    escape_in(pool, t, false, 0).is_some()
}

fn is_local_ref(pool: Types<'_>, t: Ty) -> bool {
    match pool.get(t) {
        TyData::Ref { counted, .. } => !counted,
        TyData::Opaque { borrows, .. } => borrows,
        TyData::Mut(i) => is_local_ref(pool, i),
        _ => false,
    }
}

/// Whether `from` converts to `to` by kinds alone (`ref.convert.lend`):
/// the two types are one at run time, and each reference where they
/// differ lends a counted reference as a local one, or in a function
/// type's parameter takes the local one where a counted one is passed.
/// Other positions keep their kind (`ref.type.kind-distinct`).
pub(crate) fn lends(pool: Types<'_>, from: Ty, to: Ty, polarity: i8, depth: u32) -> bool {
    if from == to {
        return true;
    }
    if depth > 32 {
        return false;
    }
    let d = depth + 1;
    let list = |a: hd_types::TyList, b: hd_types::TyList, p: i8| {
        let (a, b) = (pool.list_items(a), pool.list_items(b));
        a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| lends(pool, x, y, p, d))
    };
    match (pool.get(from), pool.get(to)) {
        (
            TyData::Ref {
                inner: a,
                mutable: ma,
                counted: ka,
            },
            TyData::Ref {
                inner: b,
                mutable: mb,
                counted: kb,
            },
        ) if ma == mb => {
            let kind = match polarity {
                1 => ka || !kb,
                -1 => kb || !ka,
                _ => ka == kb,
            };
            kind && lends(pool, a, b, 0, d)
        }
        (TyData::Mut(a), TyData::Mut(b)) => lends(pool, a, b, polarity, d),
        (TyData::Option(a), TyData::Option(b)) => lends(pool, a, b, 0, d),
        (TyData::Adt { def: x, args: a }, TyData::Adt { def: y, args: b }) if x == y => {
            list(a, b, 0)
        }
        (TyData::Tuple { elems: a, rest: ra }, TyData::Tuple { elems: b, rest: rb }) => {
            list(a, b, polarity)
                && match (ra, rb) {
                    (Some(x), Some(y)) => lends(pool, x, y, polarity, d),
                    (None, None) => true,
                    _ => false,
                }
        }
        (
            TyData::Fn {
                params: pa,
                result: ra,
                row: wa,
                suspends: sa,
                inputs: ia,
                deps: da,
            },
            TyData::Fn {
                params: pb,
                result: rb,
                row: wb,
                suspends: sb,
                inputs: ib,
                deps: db,
            },
        ) => {
            wa == wb
                && da == db
                && sa == sb
                && ia == ib
                && list(pa, pb, -polarity)
                && lends(pool, ra, rb, polarity, d)
        }
        _ => false,
    }
}

impl Ck<'_, '_> {
    /// Whether `got` converts to `want` by reference kinds alone, which
    /// needs no conversion at run time ([`lends`]).
    pub(crate) fn lends_by_kind(&self, got: Ty, want: Ty) -> bool {
        let pool = self.pool();
        let (g, w) = (
            self.infer.resolve(pool, got),
            self.infer.resolve(pool, want),
        );
        if !pool.has_ref(g) || !pool.has_ref(w) || pool.has_infer(g) || pool.has_infer(w) {
            return false;
        }
        lends(pool, g, w, 1, 0)
    }

    /// The kind step of `coerce` (`ref.convert.lend`,
    /// `ref.local.escape.counted`): a counted reference `g` lends itself
    /// where the local `w` is expected, with no conversion, and a local
    /// one where a counted one, or an optional of one, is expected is an
    /// error. `None` when neither applies.
    pub(crate) fn coerce_ref_kind(
        &mut self,
        r: Ref,
        g: Ty,
        w: Ty,
        n: NodeRef<'_>,
        what: &str,
        (target, position): (Ty, ExpectPosition),
    ) -> Option<Ref> {
        let pool = self.pool();
        let got = self.ref_counted(g)?;
        let w = self.infer.shallow(pool, w);
        let (want, optional) = match pool.get(w) {
            TyData::Ref { counted, .. } => (counted, false),
            TyData::Option(i) => match pool.get(self.infer.shallow(pool, i)) {
                TyData::Ref { counted, .. } => (counted, true),
                _ => return None,
            },
            _ => return None,
        };
        match (got, want) {
            (false, true) => {
                self.local_ref_escapes(n, Escape::of_value(what));
                Some(r)
            }
            (true, false) if !optional => {
                let (inner, mutable) = self.ref_of(g)?;
                let lent = self.ref_ty(inner, mutable, false);
                Some(self.coerce(r, lent, target, n, what, position))
            }
            _ => None,
        }
    }

    /// Reports the local reference `value` escaping to `at`.
    pub(crate) fn local_ref_escapes(&mut self, value: NodeRef<'_>, at: Escape) {
        let e = value_of(value);
        let local = self.named_local(value);
        match local.and_then(|l| self.local_ref_token(l)) {
            Some(_) => self.err(Code::LocalReferenceEscapes, value, &message(at)),
            None if local.is_some() || e.kind() == SyntaxKind::RefExpr => {
                self.err(Code::LocalReferenceEscapes, value, &box_message(at));
            }
            None => self.err(Code::LocalReferenceEscapes, value, &message(at)),
        }
    }

    /// `ref.local.escape.top-level`: a top-level binding's annotation `n`
    /// of type `t` that holds a local reference with no `&` written, as a
    /// struct with lifetimes does (`ref.lifetime.struct.holds`). A written
    /// `&` reported already ([`declarations`]).
    pub(crate) fn top_level_annotation(&mut self, t: Ty, n: NodeRef<'_>) {
        let toks = &self.cx.src.parse.tokens;
        let written = std::iter::once(n).chain(n.descendants()).any(|d| {
            d.kind() == SyntaxKind::RefType && d.direct_token(toks, TokenKind::Amp).is_some()
        });
        if !written && holds_local_ref(self.pool(), t) {
            self.err(Code::LocalReferenceEscapes, n, &message(Escape::TopLevel));
        }
    }

    /// A `*self` or `*mut self` call of `name` on a place, or on a value
    /// that is no place (`temporary`), which has no object for a counted
    /// reference to refer to (`fn.method.counted-self.place`,
    /// `fn.method.counted-self.temporary-error`).
    pub(crate) fn counted_self_on_value(&mut self, n: NodeRef<'_>, name: &str, temporary: bool) {
        let msg = if temporary {
            format!(
                "`{name}` takes a counted reference to its receiver, but a counted reference never refers to a temporary; box the value first, as `box(value).{name}()`"
            )
        } else {
            format!(
                "`{name}` takes a counted reference to its receiver, but a counted reference never refers to a variable, a field or an element; box the value at its declaration, as `x := box(value)`"
            )
        };
        self.err(Code::LocalReferenceEscapes, n, &msg);
    }

    /// The local the expression `e` names: a name, or the value of a
    /// field initializer or an argument, a shorthand `Name { x }` included.
    fn named_local(&self, e: NodeRef<'_>) -> Option<LocalId> {
        let e = crate::elem_refs::strip_parens(e);
        match e.kind() {
            SyntaxKind::NameExpr => Some(self.find_local(self.sym_of(e))?.0),
            SyntaxKind::DataFieldInit | SyntaxKind::Argument => match e.children().next() {
                Some(v) => self.named_local(v),
                None => Some(self.find_local(self.sym_of(e))?.0),
            },
            _ => None,
        }
    }

    /// The `&` of the type local `l` was declared with: a parameter's, a
    /// `&self` receiver's, an annotated binding's, or for a binding
    /// inferred from another local, that local's.
    fn local_ref_token(&self, l: LocalId) -> Option<TokenIdx> {
        if let Some(&t) = self.ref_origins.get(&l) {
            return Some(t);
        }
        let body = self.b.body();
        let syn = *body.local_syn.get(l.idx())?;
        if syn == hd_base::NodeIdx::NONE {
            return None;
        }
        let name = self.cx.names.text(*body.local_name.get(l.idx())?);
        let node = self.cx.src.parse.tree.node(syn);
        let params = match node.kind() {
            SyntaxKind::Parameter => return self.param_amp(node),
            _ => node.child(SyntaxKind::ParameterList)?,
        };
        let toks = &self.cx.src.parse.tokens;
        let p = params.children().find(|p| {
            p.direct_token(toks, TokenKind::KwSelfValue)
                .or_else(|| p.name(toks))
                .is_some_and(|t| self.cx.src.text(t) == name)
        })?;
        self.param_amp(p)
    }

    /// The `&` of a parameter's reference type, or of a `&self` receiver.
    fn param_amp(&self, p: NodeRef<'_>) -> Option<TokenIdx> {
        let toks = &self.cx.src.parse.tokens;
        p.direct_token(toks, TokenKind::Amp).or_else(|| {
            p.children()
                .find(|c| c.kind() == SyntaxKind::RefType)
                .and_then(|r| r.direct_token(toks, TokenKind::Amp))
        })
    }

    /// Records where the binding `l` of `let`/`:=` statement `at` with
    /// initializer `rhs` takes its local reference type from: its
    /// annotation's `&`, or the local `rhs` names. This keeps diagnostics
    /// for `&e` distinct from diagnostics for a declared local reference.
    pub(crate) fn note_ref_origin(&mut self, l: LocalId, at: NodeRef<'_>, rhs: NodeRef<'_>) {
        if self.ref_counted(self.b.local_ty(l)) != Some(false) {
            return;
        }
        let toks = &self.cx.src.parse.tokens;
        let written = at
            .children()
            .find(|c| c.kind() == SyntaxKind::RefType)
            .and_then(|r| r.direct_token(toks, TokenKind::Amp));
        let from = written.or_else(|| {
            let src = self.named_local(rhs)?;
            (src != l).then(|| self.local_ref_token(src))?
        });
        if let Some(t) = from {
            self.ref_origins.insert(l, t);
        }
    }

    /// `ref.local.escape.capture-stored`: the closure `n` captures a local
    /// that holds a local reference, `self` of a `&self` method included.
    /// A closure passed directly to a `fn(...)` parameter is scoped and
    /// carries those origins instead (`fn.capture.local-parameter`). Each
    /// rejected local reports once, at the outermost closure that captures
    /// it.
    pub(crate) fn local_ref_captures(
        &mut self,
        mark: hd_tir::ir::SubMark,
        n: NodeRef<'_>,
        scoped: bool,
    ) {
        let pool = self.pool();
        for l in self.b.open_captures(mark) {
            if self.ref_captured.contains(&l) {
                continue;
            }
            let t = self.infer.resolve(pool, self.b.local_ty(l));
            if holds_local_ref(pool, t) {
                if scoped {
                    continue;
                }
                self.ref_captured.insert(l);
                self.err(Code::LocalReferenceEscapes, n, &message(Escape::Capture));
            } else if pool.has_param(t) && !self.meets_static(t) {
                // `ref.generic.static.capture`: a capture of a type
                // parameter's value needs the parameter's `'static`.
                self.ref_captured.insert(l);
                let msg = format!(
                    "{}, and `{}` may hold one; its type parameter needs the bound `'static`",
                    message(Escape::Capture),
                    self.show(t)
                );
                self.err(Code::LocalReferenceEscapes, n, &msg);
            }
        }
    }

    /// Whether `t` meets `'static` (`ref.generic.static`); a goal that
    /// waits on inference meets it here.
    fn meets_static(&mut self, t: Ty) -> bool {
        let tref = hd_types::solver::TraitRef {
            trait_: self.cx.names.known.static_bound,
            self_ty: t,
            args: hd_types::TyList::EMPTY,
        };
        !matches!(self.solve(tref), Ok(hd_types::solver::Answer::Fails(_)))
    }

    /// The types the finished body inferred that keep a local reference
    /// (`ref.local.escape.type-argument`): a value's, a local's, and a
    /// local function call's type arguments
    /// (`ref.generic.local.local-function`). A written type reported
    /// already ([`declarations`]), so a type the body or its signature
    /// writes is skipped. Body and local types report once per type; local
    /// function call arguments report at every call site.
    pub(crate) fn check_local_ref_types(&mut self) {
        let mut local_calls = std::mem::take(&mut self.local_ref_call_targs);
        for (_, args, _) in &mut local_calls {
            for arg in args {
                *arg = self.zonk(*arg);
            }
        }
        let pool = self.pool();
        let declared = &self.declared_tys;
        let mut seen: HashSet<(Ty, bool)> = HashSet::default();
        let mut found: Vec<(hd_base::NodeIdx, Escape)> = Vec::new();
        let body = self.b.body();
        // A type seen at the top may still report as a type argument.
        let mut look =
            |t: Ty, top_ok: bool, skip_declared: bool, dedup_ty: bool, at: hd_base::NodeIdx| {
                if at == hd_base::NodeIdx::NONE || pool.has_poison(t) || !pool.has_ref(t) {
                    return;
                }
                if (skip_declared && declared.contains(&t))
                    || (dedup_ty && !seen.insert((t, top_ok)))
                {
                    return;
                }
                let e = if top_ok || !is_local_ref(pool, t) {
                    escape_in(pool, t, top_ok, 0)
                } else {
                    Some(Escape::TypeArgument)
                };
                if let Some(e) = e {
                    found.push((at, e));
                }
            };
        for i in 0..body.tags.len() {
            look(body.ty[i], true, true, true, body.syn[i]);
        }
        // A local function's type arguments, written or inferred
        // (`ref.generic.local.local-function`).
        for (at, args, _) in local_calls {
            for a in args {
                look(a, false, false, false, at);
            }
        }
        for (l, &t) in body.local_ty.iter().enumerate() {
            if body.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0 {
                continue;
            }
            look(t, true, true, true, body.local_syn[l]);
        }
        // One report per source position: a call and its callee start
        // at the same place.
        let mut reported = HashSet::default();
        for (at, e) in found {
            let node = self.cx.src.parse.tree.node(at);
            if !reported.insert(self.cx.src.span(node).lo) {
                continue;
            }
            self.err(Code::LocalReferenceEscapes, node, &message(e));
        }
    }
}
