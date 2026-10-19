//! Statements (02-grammar.md: Statements).

use hd_diag::Code;

use super::items::Place;
use super::{Ctx, Event, Parser};
use crate::{SyntaxKind, TokenKind};

impl Parser<'_> {
    /// One statement of an indented suite, local declarations included.
    pub(crate) fn statement(&mut self) {
        if self.keyword_as_name() {
            self.simple_statement(Ctx::Stmt);
            return;
        }
        if self.at(TokenKind::KwTests) {
            // grammar.tests.top-level
            self.error(
                Code::SyntaxError,
                "a 'tests:' block may appear only at module top level",
            );
            self.recover_line();
            return;
        }
        if self.declaration_start() {
            self.declaration(Place::Body);
            return;
        }
        if self.at(TokenKind::Colon) {
            // grammar.call.trailing-block.not-header: a second block.
            let at = self.pos();
            self.error_at(
                Code::TrailingBlockPosition,
                at,
                "a call accepts only one trailing callback block",
            );
            self.recover_line();
            return;
        }
        self.simple_statement(Ctx::Stmt);
    }

    /// A reserved word that a `:=` binds, or that an earlier failed
    /// binding in this item named (`grammar.tests.keyword`).
    fn keyword_as_name(&mut self) -> bool {
        self.current().is_some_and(|kind| {
            kind.is_keyword()
                && (self.bound_keywords.contains(&kind) || self.nth(1) == Some(TokenKind::ColonEq))
        })
    }

    /// A same-line suite body.
    pub(crate) fn inline_statement(&mut self, ctx: Ctx) {
        self.simple_statement(ctx);
    }

    fn rhs(ctx: Ctx) -> Ctx {
        if ctx == Ctx::Inline {
            Ctx::Inline
        } else {
            Ctx::Rhs
        }
    }

    pub(crate) fn simple_statement(&mut self, ctx: Ctx) {
        if self.at_word("use") && self.use_root_follows() {
            self.error_at(
                Code::SyntaxError,
                self.pos() + 1,
                "'use' goes at the top of the file or a 'tests:' block, not inside a function",
            );
        }
        match self.current() {
            Some(TokenKind::KwLet) => self.let_statement(ctx),
            Some(TokenKind::Placeholder) if self.nth(1) == Some(TokenKind::ColonEq) => {
                let marker = self.start();
                self.bump();
                self.bump();
                self.expr(Self::rhs(ctx));
                self.complete(marker, SyntaxKind::DiscardStmt);
            }
            Some(TokenKind::KwReturn | TokenKind::KwBreak) => {
                let kind = if self.at(TokenKind::KwReturn) {
                    SyntaxKind::ReturnStmt
                } else {
                    SyntaxKind::BreakStmt
                };
                let marker = self.start();
                self.bump();
                if self.can_begin_expr() {
                    self.expr(Self::rhs(ctx));
                }
                self.complete(marker, kind);
            }
            Some(TokenKind::KwContinue) => {
                self.token_node(SyntaxKind::ContinueStmt);
            }
            Some(TokenKind::KwDefer) => {
                let marker = self.start();
                let (_, end) = self.token_span(self.pos());
                self.bump();
                let error = self.errors.len();
                let same_line_expression = !self.nth_line_first(0) && self.can_begin_expr();
                if self.expect(TokenKind::Colon, "':'") {
                    self.suite(false, Ctx::Inline);
                } else if same_line_expression {
                    self.fix_error(error, "insert `:` after `defer`", &[(end, end, ":")]);
                }
                self.complete(marker, SyntaxKind::DeferStmt);
            }
            Some(TokenKind::Ident) if self.at_using() => self.using_statement(ctx),
            _ => {
                if self.at(TokenKind::KwMut)
                    && matches!(self.nth(1), Some(TokenKind::Ident | TokenKind::RawIdent))
                    && self.nth(2) == Some(TokenKind::ColonEq)
                {
                    // grammar.stmt.let-mut.only-let
                    self.error(
                        Code::SyntaxError,
                        "'mut' is only accepted by 'let': write 'let mut name = ...'",
                    );
                    self.junk();
                }
                let marker = self.start();
                let first = self.pos();
                let lhs = self.expr(ctx);
                match self.current() {
                    Some(TokenKind::ColonEq) => {
                        // grammar.stmt.short-binding.let-only
                        let at = self.pos();
                        let error = self.errors.len();
                        if matches!(ctx, Ctx::Stmt | Ctx::Inline)
                            && lhs.kind() != SyntaxKind::ParenExpr
                        {
                            self.error_at(
                                Code::MissingLet,
                                at,
                                "':=' binds one name; a pattern before it needs 'let', as in 'let (a, b) = pair'",
                            );
                        } else {
                            self.error_at(
                                Code::SyntaxError,
                                at,
                                "':=' binds one name; several names need 'let', as in 'let (a, b) = pair'",
                            );
                        }
                        if lhs.kind() == SyntaxKind::TupleExpr
                            && self.toks.kind[first..at].iter().all(|kind| {
                                matches!(kind, TokenKind::Ident | TokenKind::RawIdent | TokenKind::Placeholder | TokenKind::LParen | TokenKind::RParen | TokenKind::Comma)
                            })
                            && self.events[marker.0 + 1..].iter().all(|event| {
                                !matches!(event, Event::Start { kind, open: true, .. }
                                    if !matches!(kind, SyntaxKind::TupleExpr | SyntaxKind::NameExpr | SyntaxKind::PlaceholderExpr))
                            })
                        {
                            let (start, _) = self.token_span(first);
                            let (lo, hi) = self.token_span(at);
                            self.fix_error(
                                error,
                                "bind the tuple pattern with `let`",
                                &[(start, start, "let "), (lo, hi, "=")],
                            );
                        }
                        self.bump();
                        self.expr(Self::rhs(ctx));
                        self.complete(marker, SyntaxKind::ExprStmt);
                    }
                    Some(
                        TokenKind::Eq
                        | TokenKind::EllipsisEq
                        | TokenKind::PlusEq
                        | TokenKind::MinusEq
                        | TokenKind::StarEq
                        | TokenKind::SlashEq
                        | TokenKind::PercentEq
                        | TokenKind::AmpEq
                        | TokenKind::PipeEq
                        | TokenKind::CaretEq
                        | TokenKind::ShlEq
                        | TokenKind::ShrEq,
                    ) if !self.suite_closed => {
                        self.bump();
                        self.expr(Self::rhs(ctx));
                        self.complete(marker, SyntaxKind::AssignmentStmt);
                    }
                    _ => {
                        self.complete(marker, SyntaxKind::ExprStmt);
                    }
                }
            }
        }
    }

    /// `using` at the start of a statement, before a name on its line
    /// (grammar.stmt.using).
    fn at_using(&mut self) -> bool {
        self.at_word("using")
            && !self.nth_line_first(1)
            && matches!(self.nth(1), Some(TokenKind::Ident | TokenKind::RawIdent))
    }

    /// `using name [: T] = e` (grammar.stmt.using).
    fn using_statement(&mut self, ctx: Ctx) {
        let marker = self.start();
        self.bump(); // using
        self.token_node(SyntaxKind::BindingPattern);
        if self.eat(TokenKind::Colon) {
            self.ty();
        }
        if self.expect(TokenKind::Eq, "'='") {
            self.expr(Self::rhs(ctx));
        }
        self.complete(marker, SyntaxKind::UsingStmt);
    }

    fn let_statement(&mut self, ctx: Ctx) {
        let marker = self.start();
        self.bump(); // let
        self.pattern(true);
        if !matches!(self.current(), Some(TokenKind::Colon | TokenKind::Eq)) {
            // grammar.stmt.let-list.bare and friends.
            if self.at(TokenKind::Comma) {
                self.error(
                    Code::SyntaxError,
                    "several names after 'let' need parentheses: write 'let (a, b) = ...'",
                );
            } else {
                self.expected("':' or '=' after the pattern");
            }
            self.recover_line();
            self.complete(marker, SyntaxKind::LetStmt);
            return;
        }
        if self.eat(TokenKind::Colon) {
            self.ty();
        }
        if self.expect(TokenKind::Eq, "'='") {
            self.expr(Self::rhs(ctx));
        }
        if ctx != Ctx::Inline && self.at(TokenKind::KwElse) && !self.suite_closed {
            let clause = self.start();
            self.bump();
            if self.expect(TokenKind::Colon, "':'") {
                self.suite(false, Ctx::Inline);
            }
            self.complete(clause, SyntaxKind::ElseClause);
        }
        self.complete(marker, SyntaxKind::LetStmt);
    }
}
