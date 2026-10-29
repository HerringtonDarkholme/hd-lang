//! Local references (spec/lang/13-values-and-references.md, "Local
//! References"): `&T` and `&mut T` are lent to a call and never kept, so
//! a local reference that reaches a field, a collection element, a
//! top-level binding, a result, a closure capture, a type argument or a
//! counted reference is `local-reference-escapes`, with a fix-it that
//! writes `*` for its `&`, or for an `&e` the advice to box the value.
//!
//! Both kinds have one representation (`ref.type.kind-runtime`): they
//! lower to `Ref[T]` and `Counted[T]` (`hd_types::Types::lower_refs`),
//! which differ only in what the backend knows of the address, so
//! nothing here changes the body's code. Three checks cover the positions:
//! [`declarations`] reads the written types of a module, `coerce` reports
//! a local reference where a counted one is expected
//! ([`Ck::coerce_ref_kind`]), and the body's end reports the inferred
//! types that hold one ([`Ck::check_local_ref_types`]). Captures report
//! when their closure closes ([`Ck::local_ref_captures`]).

use hd_base::{HashSet, LocalId, TokenIdx};
use hd_diag::{Code, DiagBuf, FixSafety};
use hd_resolve::view::Src;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{Ref, Tag};
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
    /// The position as the message names it (`ref.local.escape.message`).
    fn what(self) -> &'static str {
        match self {
            Escape::Field => "a field",
            Escape::Element => "a collection element",
            Escape::TopLevel => "a top-level binding",
            Escape::Result => "a result",
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
    format!(
        "a local reference never escapes to {}; write `*` for its `&` to make it a counted reference",
        at.what()
    )
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
/// (`ref.local.escape.field`, `.top-level`, `.return`, `.type-argument`):
/// one error per `&`, with a fix-it that writes `*` for it
/// (`ref.local.escape.fix`). A parameter, a local binding and an impl's
/// target may be local references, and so may a function type's
/// parameters.
pub fn declarations(src: &Src<'_>, diags: &mut DiagBuf) {
    for item in src.root().children() {
        match item.kind() {
            // A top-level binding's annotation (`ref.local.escape.top-level`).
            SyntaxKind::LetStmt => {
                for c in item.children() {
                    if c.kind().is_type() {
                        walk_ty(src, c, Some(Escape::TopLevel), diags);
                    } else {
                        walk(src, c, diags);
                    }
                }
            }
            _ => walk(src, item, diags),
        }
    }
}

/// Every type under `n` outside a type, in its declaration's position.
fn walk(src: &Src<'_>, n: NodeRef<'_>, diags: &mut DiagBuf) {
    match n.kind() {
        SyntaxKind::DataField => {
            for c in n.children() {
                if c.kind().is_type() {
                    walk_ty(src, c, Some(Escape::Field), diags);
                } else {
                    walk(src, c, diags);
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
                                walk_ty(src, t, Some(Escape::Field), diags);
                            } else {
                                walk(src, t, diags);
                            }
                        }
                    }
                } else {
                    walk(src, c, diags);
                }
            }
        }
        // A function's or a closure's written result.
        SyntaxKind::FnDecl | SyntaxKind::ClosureExpr => {
            for c in n.children() {
                if c.kind().is_type() {
                    walk_ty(src, c, Some(Escape::Result), diags);
                } else {
                    walk(src, c, diags);
                }
            }
        }
        // An alias holds what its type holds (`ref.local.holds`), so its
        // uses report. A newtype's value is its one field.
        SyntaxKind::TypeDecl => {
            if n.direct_token(&src.parse.tokens, TokenKind::Eq).is_none() {
                for c in n.children().filter(|c| c.kind().is_type()) {
                    walk_ty(src, c, Some(Escape::Field), diags);
                }
            }
        }
        // An expression's explicit type arguments, as in `f::[&T]()`.
        SyntaxKind::TypeArgumentList => walk_ty(src, n, None, diags),
        _ if n.kind().is_type() => walk_ty(src, n, None, diags),
        _ => {
            for c in n.children() {
                walk(src, c, diags);
            }
        }
    }
}

/// The type `n` in position `at`: `None` where a local reference may
/// stand, as a parameter's type does.
fn walk_ty(src: &Src<'_>, n: NodeRef<'_>, at: Option<Escape>, diags: &mut DiagBuf) {
    match n.kind() {
        SyntaxKind::RefType => {
            let amp = n.direct_token(&src.parse.tokens, TokenKind::Amp);
            if let (Some(amp), Some(at)) = (amp, at) {
                diags.error(Code::LocalReferenceEscapes, src.span(n), &message(at));
                diags.add_fix(
                    "make it a counted reference",
                    FixSafety::Suggestion,
                    &[(src.token_span(amp), "*")],
                );
            }
            // A reference may refer to a local reference (`ref.type.group`).
            for c in n.children() {
                walk_ty(src, c, None, diags);
            }
        }
        // `ref.local.holds`: a tuple holds what its elements hold.
        SyntaxKind::TupleType | SyntaxKind::ParenType => {
            for c in n.children() {
                walk_ty(src, c, at, diags);
            }
        }
        // `ref.local.escape.type-argument`, an optional's included.
        SyntaxKind::TypeArgumentList | SyntaxKind::OptionalType => {
            for c in n.children() {
                walk_ty(src, c, Some(Escape::TypeArgument), diags);
            }
        }
        // A function type's parameters may be local; its result is a
        // result (`ref.local.escape.return`).
        SyntaxKind::FunctionType => {
            let arrow = n
                .direct_token(&src.parse.tokens, TokenKind::Arrow)
                .map(|t| src.token_span(t).lo);
            for c in n.children() {
                let result = arrow.is_some_and(|a| src.span(c).lo > a);
                let at = result.then_some(Escape::Result);
                walk_ty(src, c, at, diags);
            }
        }
        _ => {
            for c in n.children() {
                if c.kind().is_type() || c.kind() == SyntaxKind::TypeArgumentList {
                    walk_ty(src, c, None, diags);
                } else {
                    walk(src, c, diags);
                }
            }
        }
    }
}

/// Where a value of type `t` keeps a local reference: inside a type
/// argument or an optional, or as a function type's result. A local
/// reference at the top (`top_ok`), in a tuple at the top, as a function
/// type's parameter or behind a reference is no escape.
fn escape_in(pool: Types<'_>, t: Ty, top_ok: bool, depth: u32) -> Option<Escape> {
    if depth > 32 || !pool.has_ref(t) {
        return None;
    }
    let d = depth + 1;
    let list = |l, top: bool| {
        pool.list_items(l)
            .iter()
            .find_map(|&a| escape_in(pool, a, top, d))
    };
    match pool.get(t) {
        TyData::Ref {
            counted: false,
            inner,
            ..
        } => {
            if top_ok {
                escape_in(pool, inner, true, d)
            } else {
                Some(Escape::TypeArgument)
            }
        }
        TyData::Ref { inner, .. } => escape_in(pool, inner, true, d),
        TyData::Mut(i) => escape_in(pool, i, top_ok, d),
        TyData::Tuple { elems, rest } => {
            list(elems, top_ok).or_else(|| rest.and_then(|r| escape_in(pool, r, top_ok, d)))
        }
        TyData::Option(i) => escape_in(pool, i, false, d),
        TyData::Adt { args, .. } | TyData::Opaque { args, .. } => list(args, false),
        TyData::TraitValue { args, bindings, .. } => list(args, false).or_else(|| {
            bindings
                .iter()
                .find_map(|&(_, b)| escape_in(pool, b, false, d))
        }),
        TyData::Assoc { self_ty, args, .. } => {
            escape_in(pool, self_ty, false, d).or_else(|| list(args, false))
        }
        TyData::Fn { params, result, .. } => {
            list(params, true).or_else(|| match escape_in(pool, result, false, d) {
                Some(Escape::TypeArgument) if is_local_ref(pool, result) => Some(Escape::Result),
                other => other,
            })
        }
        _ => None,
    }
}

/// Whether a value of type `t` holds a local reference anywhere, its top
/// included (`ref.local.holds`), as `box` checks (`ref.box.no-local`).
pub(crate) fn holds_local_ref(pool: Types<'_>, t: Ty) -> bool {
    escape_in(pool, t, false, 0).is_some()
}

fn is_local_ref(pool: Types<'_>, t: Ty) -> bool {
    match pool.get(t) {
        TyData::Ref { counted, .. } => !counted,
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
            },
            TyData::Fn {
                params: pb,
                result: rb,
                row: wb,
                suspends: sb,
                inputs: ib,
            },
        ) => {
            wa == wb
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

    /// Reports the local reference `value` escaping to `at`, with a fix-it
    /// that writes `*` in the type it was declared with
    /// (`ref.local.escape.fix`, `ref.local.escape.fix.inferred`). A value
    /// no written `&` declares, as `&e`, gets the advice to box it and no
    /// fix-it (`ref.expr.kind.no-counted.advice`). A value that reported
    /// `element-reference-outlives-call` reports nothing more.
    pub(crate) fn local_ref_escapes(&mut self, value: NodeRef<'_>, at: Escape) {
        let e = value_of(value);
        if self.elem_kept.contains(&e.index()) {
            return;
        }
        let local = self.named_local(value);
        match local.and_then(|l| self.local_ref_token(l)) {
            Some(_) => {
                self.err(Code::LocalReferenceEscapes, value, &message(at));
                self.local_ref_fix(local.expect("a named local"));
            }
            None if local.is_some() || e.kind() == SyntaxKind::RefExpr => {
                self.err(Code::LocalReferenceEscapes, value, &box_message(at));
            }
            None => self.err(Code::LocalReferenceEscapes, value, &message(at)),
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

    fn local_ref_fix(&mut self, l: LocalId) {
        if let Some(amp) = self.local_ref_token(l) {
            let span = self.cx.src.token_span(amp);
            self.diags.add_fix(
                "make it a counted reference",
                FixSafety::Suggestion,
                &[(span, "*")],
            );
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
    /// annotation's `&`, or the local `rhs` names
    /// (`ref.local.escape.fix.inferred`).
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

    /// `ref.local.escape.capture`: the closure `n` captures a local that
    /// holds a local reference, `self` of a `&self` method included.
    /// Each local reports once, at the outermost closure that captures it.
    pub(crate) fn local_ref_captures(&mut self, mark: hd_tir::ir::SubMark, n: NodeRef<'_>) {
        let pool = self.pool();
        for l in self.b.open_captures(mark) {
            // An element reference loop's binding reports
            // `element-reference-outlives-call` instead.
            if self.ref_captured.contains(&l) || self.elem_locals.contains(&l) {
                continue;
            }
            let t = self.infer.resolve(pool, self.b.local_ty(l));
            if !is_local_ref(pool, t) && escape_in(pool, t, true, 0).is_none() {
                continue;
            }
            self.ref_captured.insert(l);
            let name = self
                .cx
                .names
                .text(self.b.body().local_name[l.idx()])
                .to_owned();
            let msg = format!(
                "the closure captures `{name}`, a local reference, which never escapes to {}; write `*` for its `&` to make it a counted reference",
                Escape::Capture.what()
            );
            self.err(Code::LocalReferenceEscapes, n, &msg);
            self.local_ref_fix(l);
        }
    }

    /// The types the finished body inferred that keep a local reference
    /// (`ref.local.escape.type-argument`, `ref.local.escape.return`): a
    /// value's, a local's, and a call's type arguments. A written type
    /// reported already ([`declarations`]), so a type the body or its
    /// signature writes is skipped, and each other type reports once.
    pub(crate) fn check_local_ref_types(&mut self) {
        let pool = self.pool();
        let declared = &self.declared_tys;
        let mut seen: HashSet<(Ty, bool)> = HashSet::default();
        let mut found: Vec<(hd_base::NodeIdx, Escape)> = Vec::new();
        let body = self.b.body();
        // A type seen at the top may still report as a type argument.
        let mut look = |t: Ty, top_ok: bool, at: hd_base::NodeIdx| {
            if at == hd_base::NodeIdx::NONE || pool.has_poison(t) || !pool.has_ref(t) {
                return;
            }
            if declared.contains(&t) || !seen.insert((t, top_ok)) {
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
            look(body.ty[i], true, body.syn[i]);
            if matches!(body.tags[i], Tag::Call | Tag::Await) {
                // `[0, def, targs]` or `[1, trait, method, self_ty, targs,
                // ...]` (`Callee::words`).
                let at = body.data[i][0] as usize + 1;
                let targs = match body.extra.get(at).copied() {
                    Some(0) => body.extra.get(at + 2).copied(),
                    Some(1) => body.extra.get(at + 4).copied(),
                    _ => None,
                };
                if let Some(l) = targs {
                    for &a in pool.list_items(hd_types::TyList(l)) {
                        look(a, false, body.syn[i]);
                    }
                }
            }
        }
        for (l, &t) in body.local_ty.iter().enumerate() {
            if body.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0 {
                continue;
            }
            look(t, true, body.local_syn[l]);
        }
        let mut reported = HashSet::default();
        for (at, e) in found {
            if !reported.insert(at) {
                continue;
            }
            // An inferred type has no `&` of its own: the reference came
            // from an `&e` or from a declared `&T`.
            let node = self.cx.src.parse.tree.node(at);
            let msg = format!(
                "a local reference never escapes to {}; box the value at its declaration, as `x := box(value)`, or write `*` for the `&` of the type it was declared with",
                e.what()
            );
            self.err(Code::LocalReferenceEscapes, node, &msg);
        }
    }
}
