//! Statements (02-grammar.md: Statements).

use hd_diag::Code;

use super::items::Place;
use super::{Ctx, Parser};
use crate::{Layout, SyntaxKind, TokenKind};

impl Parser<'_> {
    /// One statement of an indented suite, local declarations included.
    pub(crate) fn statement(&mut self) {
        if self.at(TokenKind::KwTests) {
            // grammar.tests.top-level
            self.error(Code::SyntaxError);
            self.recover_line();
            if self.eat_layout(Layout::Newline) && self.at_layout(Layout::Indent) {
                self.stray_indent_quiet();
            }
            return;
        }
        if self.declaration_start() {
            self.declaration(Place::Body);
            return;
        }
        if self.at(TokenKind::Colon) {
            // grammar.call.trailing-block.not-header: a second block.
            let at = self.pos();
            self.error_at(Code::TrailingBlockPosition, at);
            self.recover_line();
            if self.eat_layout(Layout::Newline) && self.at_layout(Layout::Indent) {
                self.stray_indent_quiet();
            }
            return;
        }
        self.simple_statement(Ctx::Stmt);
    }

    /// A same-line suite body.
    pub(crate) fn inline_statement(&mut self, ctx: Ctx) {
        self.simple_statement(ctx);
    }

    /// An indented region read as junk, without a second report.
    fn stray_indent_quiet(&mut self) {
        let marker = self.start();
        self.bump();
        let errored = self.stmt_errored;
        self.statements();
        self.stmt_errored = errored;
        self.eat_layout(Layout::Dedent);
        self.complete(marker, SyntaxKind::Error);
        self.suite_closed = true;
    }

    fn rhs(ctx: Ctx) -> Ctx {
        if ctx == Ctx::Inline {
            Ctx::Inline
        } else {
            Ctx::Rhs
        }
    }

    pub(crate) fn simple_statement(&mut self, ctx: Ctx) {
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
                self.bump();
                if self.expect(TokenKind::Colon) {
                    self.suite(false, Ctx::Inline);
                }
                self.complete(marker, SyntaxKind::DeferStmt);
            }
            _ => {
                if self.at(TokenKind::KwMut)
                    && matches!(self.nth(1), Some(TokenKind::Ident | TokenKind::RawIdent))
                    && self.nth(2) == Some(TokenKind::ColonEq)
                {
                    // grammar.stmt.let-mut.only-let
                    self.error(Code::SyntaxError);
                    self.junk();
                }
                let marker = self.start();
                let lhs = self.expr(ctx);
                match self.current() {
                    Some(TokenKind::ColonEq) => {
                        // grammar.stmt.short-binding.let-only
                        let at = self.pos();
                        let code = if matches!(ctx, Ctx::Stmt | Ctx::Inline)
                            && lhs.kind() != SyntaxKind::ParenExpr
                        {
                            Code::MissingLet
                        } else {
                            Code::SyntaxError
                        };
                        self.error_at(code, at);
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

    fn let_statement(&mut self, ctx: Ctx) {
        let marker = self.start();
        self.bump(); // let
        self.pattern(true);
        if !matches!(self.current(), Some(TokenKind::Colon | TokenKind::Eq)) {
            // grammar.stmt.let-list.bare and friends.
            self.error(Code::SyntaxError);
            self.recover_line();
            self.complete(marker, SyntaxKind::LetStmt);
            return;
        }
        if self.eat(TokenKind::Colon) {
            self.ty();
        }
        if self.expect(TokenKind::Eq) {
            self.expr(Self::rhs(ctx));
        }
        if ctx != Ctx::Inline && self.at(TokenKind::KwElse) && !self.suite_closed {
            let clause = self.start();
            self.bump();
            if self.expect(TokenKind::Colon) {
                self.suite(false, Ctx::Inline);
            }
            self.complete(clause, SyntaxKind::ElseClause);
        }
        self.complete(marker, SyntaxKind::LetStmt);
    }
}
