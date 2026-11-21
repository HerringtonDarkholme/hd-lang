//! Ranked, uncertain repairs. Probe existing trait queries without learning
//! inference state or charging the original body for a proposed edit.

use hd_base::{DefId, Span};
use hd_diag::Code;
use hd_diag::FixSafety;
use hd_resolve::BindingKind;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_types::solver::{Answer, Evidence, TraitRef};
use hd_types::{Ty, TyData};

use crate::body::Ck;

impl Ck<'_, '_> {
    pub(crate) fn suggestion_trait(&mut self, ty: Ty, trait_: DefId) -> bool {
        let pool = self.pool();
        let ty = self.infer.resolve(pool, ty);
        if pool.has_infer(ty) || pool.has_poison(ty) {
            return false;
        }
        let mark = self.infer.trial_mark();
        // BodyMemo's canonical entries survive inference rollback. Reuse
        // a separate advice memo instead of rebuilding folds per error.
        std::mem::swap(&mut self.memo, &mut self.advice_memo);
        let fuel = self.fuel;
        let answer = self.solve(TraitRef {
            trait_,
            self_ty: ty,
            args: pool.list(&[]),
        });
        self.fuel = fuel;
        std::mem::swap(&mut self.memo, &mut self.advice_memo);
        self.infer.rollback_trial(mark);
        matches!(answer, Ok(Answer::Holds { evidence, obligations, .. }) if obligations.is_empty() && !matches!(evidence, Evidence::Poison))
    }

    /// An existing Default name, or one cached-position import, only when
    /// that name is unshadowed and the value's type implements the trait.
    pub(crate) fn default_suggestion(&mut self, ty: Ty, at: Span) -> Option<Vec<(Span, String)>> {
        let def = self.cx.names.known.default;
        if !self.suggestion_trait(ty, def) {
            return None;
        }
        let sym = self.cx.names.syms.intern("Default");
        let init = self.cx.init.borrow();
        if self.find_local(sym).is_some()
            || self.cx.locals.at(sym, at.lo).is_some()
            || init.globals.contains_key(&sym)
            || init.earlier.contains_key(&sym)
        {
            return None;
        }
        drop(init);
        if let Some(binding) = self.cx.scope_at(at.lo).lookup(sym) {
            return (binding.kind == BindingKind::Item && DefId::from_raw(binding.value) == def)
                .then(Vec::new);
        }
        let insert = self.cx.src.parse.use_insertion(self.cx.src.text);
        let path = self.cx.names.path(def).replace('/', ".");
        let nl = insert.newline;
        let text = if insert.after_use {
            format!("{nl}use {path}")
        } else {
            format!("use {path}{nl}{nl}")
        };
        Some(vec![(
            Span {
                lo: insert.at,
                hi: insert.at,
                ..at
            },
            text,
        )])
    }

    /// Optional interpolation can either omit absence, expect a value, or
    /// show the optional itself. Borrow before mapping to retain !Copy values.
    pub(crate) fn optional_interpolation_fixes(
        &mut self,
        ty: Ty,
        at: Span,
        short: bool,
        before: usize,
    ) {
        if self.diags.len() != before + 1
            || self.diags.code.last() != Some(&hd_diag::Code::UnsatisfiedTraitBound)
        {
            return;
        }
        let ty = self.resolved(self.infer.resolve(self.pool(), ty));
        let Some(inner) = self.option_arg(ty) else {
            return;
        };
        let text = self.cx.src.text[at.lo as usize..at.hi as usize].to_owned();
        let value = if short {
            text.strip_prefix('$').unwrap_or(&text)
        } else {
            &text
        };
        let replacement = |text: String| if short { format!("${{{text}}}") } else { text };
        if self.suggestion_trait(inner, self.cx.names.known.display) {
            self.guess_fix(
                "format the present value, or empty text when absent",
                &[(
                    at,
                    replacement(format!(
                        r#"({value}).as_ref().map(fn(value): "$value").unwrap_or("")"#
                    )),
                )],
            );
            self.guess_fix(
                "expect a present value before formatting",
                &[(
                    at,
                    // ruled out: this is optional hd repair text, explicitly marked as a guess.
                    replacement(format!(r#"({value}).as_ref().expect("missing value")"#)),
                )],
            );
        }
        if self.standard_name("debug", self.cx.names.known.debug_fn, at.lo)
            && self.suggestion_trait(ty, self.cx.names.known.debug)
        {
            self.guess_fix(
                "format the optional with `debug`",
                &[(at, replacement(format!("debug({value})")))],
            );
        }
    }

    /// The other integer family is a policy choice too: convert the left
    /// operand, then restore an arithmetic result to the original family.
    pub(crate) fn other_integer_family_fix(&mut self, got: Ty, want: Ty, n: NodeRef<'_>) {
        let src = self.cx.src;
        let mut operand = n;
        let mut parent = src.parse.tree.parent(operand.index());
        while let Some(index) = parent {
            let node = src.parse.tree.node(index);
            if node.kind() != SyntaxKind::ParenExpr {
                break;
            }
            operand = node;
            parent = src.parse.tree.parent(index);
        }
        let Some(parent) = parent else {
            return;
        };
        let binary = src.parse.tree.node(parent);
        // A shift or power has an independent count type; converting its
        // left operand cannot repair an invalid count.
        if !matches!(
            binary.kind(),
            SyntaxKind::AdditiveExpr
                | SyntaxKind::MultiplicativeExpr
                | SyntaxKind::ComparisonExpr
                | SyntaxKind::BitwiseAndExpr
                | SyntaxKind::BitwiseOrExpr
                | SyntaxKind::BitwiseXorExpr
        ) {
            return;
        }
        let [left, right] = binary.parts();
        if right.index() != operand.index() {
            return;
        }
        let left = src.span(left);
        let whole = src.span(binary);
        let family = self.show(got);
        let result = self.show(want);
        let comparison = binary.kind() == SyntaxKind::ComparisonExpr;
        let start = Span {
            hi: left.lo,
            ..left
        };
        let end = Span {
            lo: left.hi,
            ..left
        };
        let mut edits = vec![(start, format!("{family}(")), (end, ")".into())];
        if !comparison {
            if left.lo == whole.lo {
                edits[0].1 = format!("{result}({family}(");
            } else {
                edits.push((
                    Span {
                        hi: whole.lo,
                        ..whole
                    },
                    format!("{result}("),
                ));
            }
            edits.push((
                Span {
                    lo: whole.hi,
                    ..whole
                },
                ")".into(),
            ));
        }
        self.guess_fix("calculate in the other integer family", &edits);
    }

    /// Add the missing branch without changing the branch already written.
    /// Read only the affected line's trailing whitespace/comment; the line
    /// index and import insertion point are each built once per source file.
    pub(crate) fn missing_else_fixes(&mut self, statement: NodeRef<'_>, want: Ty) {
        let mut node = statement;
        while matches!(node.kind(), SyntaxKind::ExprStmt | SyntaxKind::ParenExpr) {
            let Some(child) = node.children().next() else {
                return;
            };
            node = child;
        }
        if node.kind() != SyntaxKind::IfExpr
            || node
                .children()
                .any(|child| child.kind() == SyntaxKind::ElseClause)
        {
            return;
        }
        let src = self.cx.src;
        let span = src.span(node);
        let [_, first_branch] = node.parts();
        let first = src.span(first_branch);
        let (line, column) = src.parse.line_column(src.text, span.lo as usize);
        let same_line = line == src.parse.line_column(src.text, first.lo as usize).0;
        let mut at = span.hi as usize;
        if !same_line {
            let bytes = src.text.as_bytes();
            let mut end = at;
            while bytes.get(end).is_some_and(|b| matches!(b, b' ' | b'\t')) {
                end += 1;
            }
            if bytes.get(end) == Some(&b'#') {
                while bytes.get(end).is_some_and(|b| !matches!(b, b'\r' | b'\n')) {
                    end += 1;
                }
            }
            if end == bytes.len() || bytes.get(end).is_some_and(|b| matches!(b, b'\r' | b'\n')) {
                at = end;
            }
        }
        let Ok(at) = u32::try_from(at) else {
            return;
        };
        let insert = Span {
            lo: at,
            hi: at,
            ..span
        };
        let leader = if same_line {
            " else: ".into()
        } else {
            format!(
                "{}{}else: ",
                src.parse.use_insertion(src.text).newline,
                " ".repeat(column - 1)
            )
        };
        if let Some(mut imports) = self.default_suggestion(want, span) {
            imports.push((insert, format!("{leader}Default::default()")));
            self.guess_fix(
                "use the result type's default when the condition is false",
                &imports,
            );
        }
        let panic = self.cx.names.member(
            DefId::from_raw(self.cx.names.known.core_module.raw()),
            hd_intern::PathKind::Item,
            "panic",
        );
        if self.standard_name("panic", panic, span.lo) {
            self.guess_fix(
                "reject the missing path explicitly",
                &[(insert, format!(r#"{leader}panic("missing result")"#))],
            );
        }
    }

    pub(crate) fn result_optional_fixes(
        &mut self,
        got: Ty,
        inner: Ty,
        n: NodeRef<'_>,
        before: usize,
    ) {
        if self.diags.len() != before + 1 || self.diags.code.last() != Some(&Code::TypeMismatch) {
            return;
        }
        let pool = self.pool();
        let got = self.resolved(self.infer.resolve(pool, got));
        let TyData::Adt { def, args } = pool.get(got) else {
            return;
        };
        if def != self.cx.names.known.result {
            return;
        }
        let &[ok, error] = pool.list_items(args) else {
            return;
        };
        if !self.can_unify(ok, inner) {
            return;
        }
        let span = self.cx.src.span(n);
        let start = Span {
            hi: span.lo,
            ..span
        };
        let end = Span {
            lo: span.hi,
            ..span
        };
        self.guess_fix(
            "turn an error into an absent value",
            &[(start, "(".into()), (end, ").ok()".into())],
        );
        if self.suggestion_trait(error, self.cx.names.known.display) {
            self.guess_fix(
                "expect success and keep the present value",
                &[
                    (start, "(".into()),
                    // ruled out: this is optional hd repair text, explicitly marked as a guess.
                    (end, r#").expect("operation failed")"#.into()),
                ],
            );
        }
    }

    /// Preserve a propagated success value, or explicitly ignore the
    /// operation when the whole statement's value is unused. Parent links
    /// are indexed once by the parser, so no body scan finds the statement.
    pub(crate) fn void_try_fixes(&mut self, t: Ty, ret: Ty, n: NodeRef<'_>, e: NodeRef<'_>) {
        let pool = self.pool();
        if self.resolved(self.infer.resolve(pool, ret)) != Ty::VOID {
            return;
        }
        let t = self.resolved(self.infer.resolve(pool, t));
        let (payload, error) = match pool.get(t) {
            TyData::Adt { def, args } if def == self.cx.names.known.result => {
                let &[ok, error] = pool.list_items(args) else {
                    return;
                };
                (ok, Some(error))
            }
            _ => match self.option_arg(t) {
                Some(payload) => (payload, None),
                None => return,
            },
        };
        let src = self.cx.src;
        let Some(question) = n.direct_token(&src.parse.tokens, TokenKind::Question) else {
            return;
        };
        let question = src.token_span(question);
        let expression = src.span(e);
        let mut parent = src.parse.tree.parent(n.index());
        while let Some(index) = parent {
            if src.parse.tree.node(index).kind() != SyntaxKind::ParenExpr {
                break;
            }
            parent = src.parse.tree.parent(index);
        }
        let statement = parent
            .map(|index| src.parse.tree.node(index))
            .filter(|parent| parent.kind() == SyntaxKind::ExprStmt)
            .map(|parent| src.span(parent));
        let prefix = Span {
            hi: expression.lo,
            ..expression
        };
        let mut edits = Vec::new();
        if let Some(statement) = statement {
            if statement.lo == expression.lo {
                edits.push((prefix, "_ := (".into()));
            } else {
                edits.push((
                    Span {
                        hi: statement.lo,
                        ..statement
                    },
                    "_ := ".into(),
                ));
                edits.push((prefix, "(".into()));
            }
        } else {
            edits.push((prefix, "(".into()));
        }
        let display =
            error.is_none_or(|error| self.suggestion_trait(error, self.cx.names.known.display));
        let expect = if display {
            // ruled out: this is optional hd repair text, explicitly marked as a guess.
            r#").expect("operation failed")"#
        } else {
            // ruled out: this is optional hd repair text, explicitly marked as a guess.
            r#").map_err(fn(_error): "").expect("operation failed")"#
        };
        edits.push((question, expect.into()));
        self.guess_fix("expect success instead of propagating failure", &edits);
        if let Some(statement) = statement {
            self.guess_fix(
                "explicitly discard the operation's result",
                &[
                    (
                        Span {
                            hi: statement.lo,
                            ..statement
                        },
                        "_ := ".into(),
                    ),
                    (question, String::new()),
                ],
            );
        } else if let Some(mut imports) = self.default_suggestion(payload, expression) {
            imports.extend([
                (prefix, "(".into()),
                (question, ").unwrap_or(Default::default())".into()),
            ]);
            self.guess_fix("use the success type's default on failure", &imports);
        }
    }

    pub(crate) fn list_element_append_fixes(
        &mut self,
        (receiver, want, got): (Ty, Ty, Ty),
        args: &crate::call::Args<'_>,
        n: NodeRef<'_>,
        before: usize,
    ) {
        if self.diags.len() != before + 1 || self.diags.code.last() != Some(&Code::TypeMismatch) {
            return;
        }
        let pool = self.pool();
        let mut receiver = self.resolved(self.infer.resolve(pool, receiver));
        while let Some((inner, _)) = self.ref_of(receiver) {
            receiver = self.resolved(inner);
        }
        let TyData::Adt { def, .. } = pool.get(receiver) else {
            return;
        };
        if def != self.cx.names.known.list {
            return;
        }
        let TyData::Ref {
            inner,
            mutable: true,
            counted: false,
        } = pool.get(self.resolved(want))
        else {
            return;
        };
        let TyData::Adt { def, args: types } = pool.get(self.resolved(inner)) else {
            return;
        };
        if def != self.cx.names.known.list {
            return;
        }
        let &[element] = pool.list_items(types) else {
            return;
        };
        if !self.can_unify(got, element)
            || args.spread.is_some()
            || args.trailing.is_some()
            || !args.after_spread.is_empty()
        {
            return;
        }
        let (value, named) = match (args.positional.as_slice(), args.named.as_slice()) {
            ([value], []) => (*value, false),
            ([], [(_, value)]) => (*value, true),
            _ => return,
        };
        let src = self.cx.src;
        let Some(callee) = n.children().next() else {
            return;
        };
        let Some(method) = callee.name(&src.parse.tokens) else {
            return;
        };
        let mut push = vec![(src.token_span(method), "push".into())];
        if named {
            let Some(list) = n.child(SyntaxKind::ArgumentList) else {
                return;
            };
            let Some(argument) = list
                .children()
                .find(|child| child.kind() == SyntaxKind::NamedArgument)
            else {
                return;
            };
            let Some(name) = argument.name(&src.parse.tokens) else {
                return;
            };
            push.push((src.token_span(name), "value".into()));
        }
        self.guess_fix("push one element instead of appending a list", &push);
        let span = src.span(value);
        self.guess_fix(
            "append a writable singleton list",
            &[
                (
                    Span {
                        hi: span.lo,
                        ..span
                    },
                    "&mut [".into(),
                ),
                (
                    Span {
                        lo: span.hi,
                        ..span
                    },
                    "]".into(),
                ),
            ],
        );
    }

    /// One root-domain repair per match, attached to its first unknown
    /// variant. Do not descend into payload patterns, whose enums differ.
    pub(crate) fn result_pattern_fixes(&mut self, ty: Ty, pattern: NodeRef<'_>) {
        let src = self.cx.src;
        let Some(arm) = src
            .parse
            .tree
            .parent(pattern.index())
            .map(|i| src.parse.tree.node(i))
        else {
            return;
        };
        if arm.kind() != SyntaxKind::MatchArm {
            return;
        }
        let Some(matched) = src
            .parse
            .tree
            .parent(arm.index())
            .map(|i| src.parse.tree.node(i))
        else {
            return;
        };
        if matched.kind() != SyntaxKind::MatchExpr {
            return;
        }
        let pool = self.pool();
        let mut ty = self.resolved(self.infer.resolve(pool, ty));
        let mut borrowed = false;
        while let Some((inner, _)) = self.ref_of(ty) {
            ty = self.resolved(inner);
            borrowed = true;
        }
        if !matches!(pool.get(ty), TyData::Adt { def, .. } if def == self.cx.names.known.result) {
            return;
        }
        let mut edits = Vec::new();
        for arm in matched
            .children()
            .filter(|node| node.kind() == SyntaxKind::MatchArm)
        {
            let Some(pattern) = arm.children().next() else {
                return;
            };
            if matches!(
                pattern.kind(),
                SyntaxKind::WildcardPattern | SyntaxKind::BindingPattern
            ) {
                continue;
            }
            if pattern.kind() != SyntaxKind::VariantPattern {
                return;
            }
            let Some(name) = pattern
                .direct_tokens()
                .filter(|&token| {
                    matches!(
                        src.tkind(token),
                        Some(TokenKind::Ident | TokenKind::RawIdent)
                    )
                })
                .last()
            else {
                return;
            };
            let span = src.span(pattern);
            let args = pattern.child(SyntaxKind::PatternArgumentList);
            match src.text(name) {
                "Some" if args.is_some_and(|list| list.children().count() == 1) => {
                    edits.push((
                        Span {
                            hi: src.token_span(name).hi,
                            ..span
                        },
                        ".Ok".into(),
                    ));
                }
                "None" if args.is_none_or(|list| list.children().next().is_none()) => {
                    edits.push((span, ".Err(_)".into()));
                }
                // A match mixing enum domains or payload arities has no
                // single coherent domain conversion to suggest.
                _ => return,
            }
        }
        if edits.is_empty() {
            return;
        }
        self.guess_fix("match the Result's success and error variants", &edits);
        let [value] = matched.parts();
        let span = src.span(value);
        self.guess_fix(
            "turn the Result into an optional before matching",
            &[
                (
                    Span {
                        hi: span.lo,
                        ..span
                    },
                    "(".into(),
                ),
                (
                    Span {
                        lo: span.hi,
                        ..span
                    },
                    if borrowed {
                        ").as_ref().ok()"
                    } else {
                        ").ok()"
                    }
                    .into(),
                ),
            ],
        );
    }

    /// Preserve a fresh box's writable view, or make a separate writable
    /// copy. Changing a borrowed object's permission requires evidence of
    /// the original writable allocation; an arbitrary readonly alias is
    /// never promoted merely by editing its annotation.
    pub(crate) fn mutable_reference_fixes(&mut self, inner: Ty, n: NodeRef<'_>) {
        let src = self.cx.src;
        let core = DefId::from_raw(self.cx.names.known.core_module.raw());
        let boxed = self.cx.names.member(core, hd_intern::PathKind::Item, "box");
        if let Some(at) = self.fresh_box_annotation(n, boxed) {
            self.guess_fix(
                "retain the fresh object's writable reference",
                &[(at, "mut ".into())],
            );
        }
        let span = src.span(n);
        // `trait.dyn.reference-no-bare-read`: a dyn referent cannot become a bare
        // value, even if its reference has Copy evidence.
        if !matches!(
            self.pool().get(self.resolved(inner)),
            TyData::TraitValue { .. }
        ) && self.standard_name("box", boxed, span.lo)
            && self.suggestion_trait(inner, self.cx.names.known.copy)
        {
            self.guess_fix(
                "provide a separate writable copy",
                &[
                    (
                        Span {
                            hi: span.lo,
                            ..span
                        },
                        "box(*(".into(),
                    ),
                    (
                        Span {
                            lo: span.hi,
                            ..span
                        },
                        "))".into(),
                    ),
                ],
            );
        }
    }

    fn fresh_box_annotation(&self, mut value: NodeRef<'_>, boxed: DefId) -> Option<Span> {
        let src = self.cx.src;
        while value.kind() == SyntaxKind::ParenExpr {
            value = value.children().next()?;
        }
        if value.kind() != SyntaxKind::NameExpr {
            return None;
        }
        let (local, _) = self.find_local(self.sym_of(value))?;
        let syn = self.b.body().local_syn[local.idx()];
        if syn == hd_base::NodeIdx::NONE {
            return None;
        }
        let binding = src.parse.tree.node(src.parse.tree.parent(syn)?);
        if binding.kind() != SyntaxKind::LetStmt {
            return None;
        }
        let (_, annotation, initializer) = crate::pat::binding_parts(binding);
        if initializer.kind() != SyntaxKind::CallExpr {
            return None;
        }
        let callee = initializer.children().next()?;
        if callee.kind() != SyntaxKind::NameExpr
            || src.text(src.first(callee)) != "box"
            || !self.standard_name("box", boxed, src.span(callee).lo)
        {
            return None;
        }
        let annotation = annotation.filter(|node| node.kind() == SyntaxKind::RefType)?;
        let marker = annotation
            .direct_token(&src.parse.tokens, TokenKind::Star)
            .or_else(|| annotation.direct_token(&src.parse.tokens, TokenKind::Amp))?;
        let span = src.token_span(marker);
        Some(Span {
            lo: span.hi,
            ..span
        })
    }

    pub(crate) fn guess_fix(&mut self, title: &str, edits: &[(Span, String)]) {
        let edits: Vec<_> = edits
            .iter()
            .map(|(span, text)| (*span, text.as_str()))
            .collect();
        self.diags.add_fix(title, FixSafety::Suggestion, &edits);
    }
}
