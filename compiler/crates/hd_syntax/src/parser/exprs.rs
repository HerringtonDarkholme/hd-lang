//! Expressions (02-grammar.md: Expressions, Control-Flow Expressions,
//! Comprehensions, Requirements And Provider Contexts). Binary operators
//! climb by binding power; each level keeps its own node kind.

use hd_diag::Code;

use super::{Ctx, Done, Parser};
use crate::layout::SuiteKind;
use crate::{Layout, SyntaxKind, TokenKind};

/// Binding power and node of an infix operator.
fn infix(kind: TokenKind) -> Option<(u8, SyntaxKind)> {
    Some(match kind {
        TokenKind::OrOr => (1, SyntaxKind::LogicalOrExpr),
        TokenKind::AndAnd => (2, SyntaxKind::LogicalAndExpr),
        TokenKind::EqEq
        | TokenKind::NotEq
        | TokenKind::Lt
        | TokenKind::LtEq
        | TokenKind::Gt
        | TokenKind::GtEq
        | TokenKind::KwIs => (3, SyntaxKind::ComparisonExpr),
        TokenKind::PipeGt => (4, SyntaxKind::PipeExpr),
        TokenKind::Pipe => (5, SyntaxKind::BitwiseOrExpr),
        TokenKind::Caret => (6, SyntaxKind::BitwiseXorExpr),
        TokenKind::Amp => (7, SyntaxKind::BitwiseAndExpr),
        TokenKind::Shl | TokenKind::Shr => (8, SyntaxKind::ShiftExpr),
        TokenKind::Plus | TokenKind::Minus => (9, SyntaxKind::AdditiveExpr),
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => {
            (10, SyntaxKind::MultiplicativeExpr)
        }
        _ => return None,
    })
}

/// Postfix-expression kinds: the callee of a trailing block call.
fn is_postfix(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::NameExpr
            | SyntaxKind::FieldExpr
            | SyntaxKind::CallExpr
            | SyntaxKind::PathExpr
            | SyntaxKind::TypeArgsExpr
            | SyntaxKind::IndexExpr
    )
}

impl Parser<'_> {
    pub(crate) fn can_begin_expr(&mut self) -> bool {
        self.current().is_some_and(|kind| {
            matches!(
                kind,
                TokenKind::Ident
                    | TokenKind::RawIdent
                    | TokenKind::Placeholder
                    | TokenKind::Number
                    | TokenKind::String
                    | TokenKind::StrHead
                    | TokenKind::Char
                    | TokenKind::LParen
                    | TokenKind::LBracket
                    | TokenKind::LBrace
                    | TokenKind::Dot
                    | TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Bang
                    | TokenKind::Tilde
                    | TokenKind::Dollar
                    | TokenKind::DotDot
                    | TokenKind::DotDotEq
                    | TokenKind::KwSelfType
                    | TokenKind::KwSelfValue
                    | TokenKind::KwTrue
                    | TokenKind::KwFalse
                    | TokenKind::KwPass
                    | TokenKind::KwIf
                    | TokenKind::KwFor
                    | TokenKind::KwWhile
                    | TokenKind::KwMatch
                    | TokenKind::KwFn
            )
        })
    }

    /// The token may begin a range's end bound (`grammar.expr.range.open-end`).
    fn can_begin_operand(&mut self) -> bool {
        self.can_begin_expr()
            && !matches!(
                self.current(),
                Some(
                    TokenKind::KwIf
                        | TokenKind::KwFor
                        | TokenKind::KwWhile
                        | TokenKind::KwMatch
                        | TokenKind::KwFn
                        | TokenKind::DotDot
                        | TokenKind::DotDotEq
                )
            )
    }

    pub(crate) fn prefixed_string(&mut self, n: usize) -> bool {
        self.nth_text(n)
            .as_bytes()
            .first()
            .is_some_and(|byte| *byte != b'"')
    }

    pub(crate) fn expr(&mut self, ctx: Ctx) -> Done {
        if !self.enter() {
            let marker = self.start();
            self.skip_balanced();
            return self.complete(marker, SyntaxKind::Error);
        }
        let done = self.expr_inner(ctx);
        self.leave();
        done
    }

    fn expr_inner(&mut self, ctx: Ctx) -> Done {
        if matches!(self.current(), Some(TokenKind::Ident | TokenKind::RawIdent))
            && self.nth(1) == Some(TokenKind::ColonEq)
        {
            let marker = self.start();
            self.token_node(SyntaxKind::BindingPattern);
            self.bump(); // :=
            let inner = if ctx == Ctx::Rhs { Ctx::Closed } else { ctx };
            self.expr(inner);
            return self.complete(marker, SyntaxKind::BindingExpr);
        }
        if self.suite_start() {
            let bad = match ctx {
                Ctx::Closed => true,
                Ctx::Inline => matches!(self.current(), Some(TokenKind::KwIf | TokenKind::KwMatch)),
                _ => false,
            };
            if bad {
                // grammar.closed.*, grammar.inline.no-if
                self.error(Code::SyntaxError);
            }
            let done = self.suite_expr(ctx);
            if ctx == Ctx::Continued && self.suite_inline {
                // grammar.continued.no-same-line-suite
                self.error(Code::SyntaxError);
            }
            return done;
        }
        let done = self.range_expr();
        if matches!(ctx, Ctx::Stmt | Ctx::Rhs) && self.at(TokenKind::Colon) {
            return self.trailing_block(done, ctx);
        }
        done
    }

    /// `postfix_expression ":" indented_suite_body`.
    fn trailing_block(&mut self, callee: Done, ctx: Ctx) -> Done {
        if !is_postfix(callee.kind()) {
            // grammar.call.trailing-block.accepted
            self.error(Code::SyntaxError);
            return callee;
        }
        if !self.nth_line_first(1) && self.nth(1).is_some() {
            // grammar.call.trailing-block.next-line, grammar.stmt.typed-binding.let
            let code = if ctx == Ctx::Stmt && callee.kind() == SyntaxKind::NameExpr {
                Code::MissingLet
            } else {
                Code::SyntaxError
            };
            self.error(code);
            self.recover_line();
            return callee;
        }
        let marker = self.precede(callee);
        self.bump(); // :
        self.suite(false, Ctx::Inline);
        self.complete(marker, SyntaxKind::TrailingCallExpr)
    }

    fn suite_start(&mut self) -> bool {
        match self.current() {
            Some(TokenKind::KwIf | TokenKind::KwFor | TokenKind::KwWhile | TokenKind::KwMatch) => {
                true
            }
            Some(TokenKind::KwFn) => match self.nth(1) {
                Some(TokenKind::LParen) => true,
                Some(TokenKind::Bang) => self.nth(2) == Some(TokenKind::LParen),
                _ => false,
            },
            Some(TokenKind::Dollar) => {
                self.nth(1) == Some(TokenKind::Dot) && self.nth_text(2) == "with"
            }
            _ => false,
        }
    }

    fn suite_expr(&mut self, ctx: Ctx) -> Done {
        match self.current() {
            Some(TokenKind::KwIf) => self.if_expr(ctx),
            Some(TokenKind::KwFor) => self.for_expr(ctx),
            Some(TokenKind::KwWhile) => self.while_expr(ctx),
            Some(TokenKind::KwMatch) => self.match_expr(ctx),
            Some(TokenKind::KwFn) => self.closure(),
            _ => self.context_expr(),
        }
    }

    /// `else` continues the construct: directly, or on the next line after
    /// a same-line suite (whose `SUITE_END` replaces the `NEWLINE`).
    fn at_else(&mut self) -> bool {
        if self.at(TokenKind::KwElse) {
            return true;
        }
        if self.suite_closed
            && self.suite_inline
            && self.at_layout(Layout::Newline)
            && self.cur.pending() == [Layout::Newline]
            && self.toks.kind.get(self.pos()) == Some(&TokenKind::KwElse)
        {
            self.cur.bump();
            return true;
        }
        false
    }

    fn if_expr(&mut self, ctx: Ctx) -> Done {
        let marker = self.start();
        self.bump(); // if
        self.expr(ctx.header());
        if self.expect(TokenKind::Colon) {
            self.suite(false, Ctx::Inline);
            if self.at_else() {
                let clause = self.start();
                self.bump(); // else
                if self.at(TokenKind::KwIf) {
                    self.if_expr(ctx);
                } else if self.expect(TokenKind::Colon) {
                    self.suite(false, Ctx::Inline);
                }
                self.complete(clause, SyntaxKind::ElseClause);
            }
        }
        self.complete(marker, SyntaxKind::IfExpr)
    }

    fn loop_else(&mut self) {
        if self.at_else() {
            let clause = self.start();
            self.bump();
            if self.expect(TokenKind::Colon) {
                self.suite(false, Ctx::Inline);
            }
            self.complete(clause, SyntaxKind::ElseClause);
        }
    }

    fn for_expr(&mut self, ctx: Ctx) -> Done {
        let marker = self.start();
        self.bump(); // for
        self.pattern(false);
        if self.expect(TokenKind::KwIn) {
            self.expr(ctx.header());
            if self.expect(TokenKind::Colon) {
                self.suite(false, Ctx::Inline);
                self.loop_else();
            }
        }
        self.complete(marker, SyntaxKind::ForExpr)
    }

    fn while_expr(&mut self, ctx: Ctx) -> Done {
        let marker = self.start();
        self.bump(); // while
        self.expr(ctx.header());
        if self.expect(TokenKind::Colon) {
            self.suite(false, Ctx::Inline);
            self.loop_else();
        }
        self.complete(marker, SyntaxKind::WhileExpr)
    }

    fn match_expr(&mut self, ctx: Ctx) -> Done {
        let marker = self.start();
        self.bump(); // match
        self.expr(ctx.header());
        if self.expect(TokenKind::Colon) {
            match self.cur.open_suite(false) {
                SuiteKind::Indented => {
                    self.eat_layout(Layout::Newline);
                    self.eat_layout(Layout::Indent);
                    self.match_arms();
                    self.eat_layout(Layout::Dedent);
                }
                SuiteKind::SameLine => {
                    // A match takes an indented arm list.
                    self.cur.close_inline();
                    self.error(Code::SyntaxError);
                    self.recover_line();
                }
                SuiteKind::Missing => self.error(Code::SyntaxError),
            }
        }
        self.suite_closed = true;
        self.suite_inline = false;
        self.complete(marker, SyntaxKind::MatchExpr)
    }

    fn match_arms(&mut self) {
        let errored = self.stmt_errored;
        loop {
            match self.layout() {
                Some(Layout::Dedent) => break,
                Some(Layout::Newline) => {
                    self.bump();
                    continue;
                }
                Some(Layout::Indent) => {
                    self.stray_indent();
                    continue;
                }
                _ => {}
            }
            if self.at_eof() {
                break;
            }
            let before = self.pos();
            self.stmt_errored = false;
            self.match_arm();
            self.statement_end();
            if self.pos() == before && self.layout().is_none() && !self.at_eof() {
                self.junk();
            }
        }
        self.stmt_errored = errored;
    }

    fn match_arm(&mut self) {
        let marker = self.start();
        self.pattern(false);
        if self.at(TokenKind::KwIf) {
            let guard = self.start();
            self.bump();
            self.expr(Ctx::Closed);
            self.complete(guard, SyntaxKind::MatchGuard);
        }
        if self.expect(TokenKind::FatArrow) {
            let body = self.start();
            match self.cur.open_arm() {
                SuiteKind::Indented => {
                    self.indented_body();
                    self.suite_closed = true;
                    self.suite_inline = false;
                }
                SuiteKind::SameLine => self.simple_statement(Ctx::Stmt),
                SuiteKind::Missing => self.error(Code::SyntaxError),
            }
            self.complete(body, SyntaxKind::Block);
        } else {
            self.recover_line();
        }
        self.complete(marker, SyntaxKind::MatchArm);
    }

    /// `fn [!] (params) [-> result] [$ row] : suite`.
    fn closure(&mut self) -> Done {
        let marker = self.start();
        self.bump(); // fn
        if self.at(TokenKind::Bang) {
            self.bump();
        }
        self.param_list(super::items::Params::Closure);
        if self.eat(TokenKind::Arrow) {
            self.ty_result();
        }
        if self.at(TokenKind::Dollar) {
            self.requirement_clause();
        }
        if self.expect(TokenKind::Colon) {
            self.suite(true, Ctx::Inline);
        }
        self.complete(marker, SyntaxKind::ClosureExpr)
    }

    /// `$.use(...)`, `$.context(...)`, `$.with(...): suite`, `$.Context[...]`.
    fn context_expr(&mut self) -> Done {
        let marker = self.start();
        self.bump(); // $
        if !self.expect(TokenKind::Dot) {
            return self.complete(marker, SyntaxKind::ContextExpr);
        }
        let word = self.nth_text(0);
        if !self.at(TokenKind::Ident) {
            self.error(Code::SyntaxError);
            return self.complete(marker, SyntaxKind::ContextExpr);
        }
        self.bump();
        match word {
            "use" => {
                if self.expect(TokenKind::LParen) {
                    while !self.at(TokenKind::RParen) && self.current().is_some() {
                        let before = self.pos();
                        self.ty_bound_trait(true);
                        if !self.eat(TokenKind::Comma) || self.pos() == before {
                            break;
                        }
                    }
                    self.expect(TokenKind::RParen);
                }
            }
            "context" | "with" => {
                if self.at(TokenKind::LParen) {
                    self.context_entries();
                } else {
                    self.error(Code::SyntaxError);
                }
                if word == "with" && self.expect(TokenKind::Colon) {
                    self.suite(false, Ctx::Inline);
                }
            }
            "Context" => {
                if self.expect(TokenKind::LBracket) {
                    self.ty_argument();
                    self.expect(TokenKind::RBracket);
                }
            }
            _ => self.error(Code::SyntaxError),
        }
        self.complete(marker, SyntaxKind::ContextExpr)
    }

    fn context_entries(&mut self) {
        let list = self.start();
        self.bump(); // (
        while !self.at(TokenKind::RParen) && self.current().is_some() {
            let before = self.pos();
            let entry = self.start();
            if self.key_entry_ahead() {
                self.ty_bound_trait(true);
                if self.expect(TokenKind::Eq) {
                    self.expr(Ctx::Expr);
                }
            } else {
                if self.at(TokenKind::Ellipsis) {
                    // grammar.primary.prefix-elsewhere
                    self.error(Code::SyntaxError);
                    self.bump();
                }
                let value = self.expr(Ctx::Continued);
                if self.at(TokenKind::Ellipsis) {
                    let spread = self.precede(value);
                    self.bump();
                    self.complete(spread, SyntaxKind::SpreadExpr);
                } else {
                    self.error(Code::SyntaxError);
                }
            }
            self.complete(entry, SyntaxKind::ContextEntry);
            if !self.eat(TokenKind::Comma) || self.pos() == before {
                break;
            }
        }
        self.expect(TokenKind::RParen);
        self.complete(list, SyntaxKind::ArgumentList);
    }

    /// `[mut] Name.Name [args] =` ahead: a provider entry with a key.
    fn key_entry_ahead(&mut self) -> bool {
        let mut index = self.pos();
        let kinds = &self.toks.kind;
        if kinds.get(index) == Some(&TokenKind::KwMut) {
            index += 1;
        }
        if !matches!(
            kinds.get(index),
            Some(TokenKind::Ident | TokenKind::RawIdent)
        ) {
            return false;
        }
        index += 1;
        while kinds.get(index) == Some(&TokenKind::Dot)
            && matches!(
                kinds.get(index + 1),
                Some(TokenKind::Ident | TokenKind::RawIdent)
            )
        {
            index += 2;
        }
        if kinds.get(index) == Some(&TokenKind::LBracket) {
            let mut depth = 0_u32;
            while let Some(kind) = kinds.get(index) {
                if kind.opens() {
                    depth += 1;
                } else if kind.closes() {
                    depth -= 1;
                    if depth == 0 {
                        index += 1;
                        break;
                    }
                }
                index += 1;
            }
        }
        kinds.get(index) == Some(&TokenKind::Eq)
    }

    // ----------------------------------------------------------- operators

    fn range_expr(&mut self) -> Done {
        if matches!(
            self.current(),
            Some(TokenKind::DotDot | TokenKind::DotDotEq)
        ) {
            let marker = self.start();
            let inclusive = self.at(TokenKind::DotDotEq);
            self.bump();
            if self.can_begin_operand() {
                self.binary(0);
            } else if inclusive {
                // grammar.expr.range.inclusive-needs-end
                self.error(Code::SyntaxError);
            }
            return self.complete(marker, SyntaxKind::RangeExpr);
        }
        let lhs = self.binary(0);
        if matches!(
            self.current(),
            Some(TokenKind::DotDot | TokenKind::DotDotEq)
        ) {
            let marker = self.precede(lhs);
            let inclusive = self.at(TokenKind::DotDotEq);
            self.bump();
            if self.can_begin_operand() {
                self.binary(0);
            } else if inclusive {
                self.error(Code::SyntaxError);
            }
            let done = self.complete(marker, SyntaxKind::RangeExpr);
            if matches!(
                self.current(),
                Some(TokenKind::DotDot | TokenKind::DotDotEq)
            ) {
                // grammar.expr.range.non-assoc
                self.error(Code::SyntaxError);
            }
            return done;
        }
        lhs
    }

    fn binary(&mut self, min: u8) -> Done {
        let mut lhs = self.unary();
        while let Some((power, kind)) = self.current().and_then(infix) {
            if power < min {
                break;
            }
            let marker = self.precede(lhs);
            self.bump();
            self.binary(power + 1);
            lhs = self.complete(marker, kind);
            if kind == SyntaxKind::ComparisonExpr
                && self
                    .current()
                    .and_then(infix)
                    .is_some_and(|(_, next)| next == SyntaxKind::ComparisonExpr)
            {
                // grammar.expr.no-comparison-chain
                let at = self.pos();
                self.error_at(Code::ComparisonChaining, at);
            }
        }
        lhs
    }

    fn unary(&mut self) -> Done {
        if matches!(
            self.current(),
            Some(TokenKind::Plus | TokenKind::Minus | TokenKind::Bang | TokenKind::Tilde)
        ) {
            if !self.enter() {
                let marker = self.start();
                self.skip_balanced();
                return self.complete(marker, SyntaxKind::Error);
            }
            let marker = self.start();
            self.bump();
            self.unary();
            self.leave();
            return self.complete(marker, SyntaxKind::UnaryExpr);
        }
        let base = self.postfix();
        if self.at(TokenKind::StarStar) {
            let marker = self.precede(base);
            self.bump();
            if !self.enter() {
                self.skip_balanced();
                return self.complete(marker, SyntaxKind::PowerExpr);
            }
            self.unary();
            self.leave();
            return self.complete(marker, SyntaxKind::PowerExpr);
        }
        base
    }

    fn postfix(&mut self) -> Done {
        let mut lhs = self.primary();
        while let Some(kind) = self.current() {
            let same_line = !self.nth_line_first(0);
            match kind {
                TokenKind::Dot => {
                    match self.nth(1) {
                        Some(TokenKind::Ident | TokenKind::RawIdent) => {
                            let marker = self.precede(lhs);
                            self.bump();
                            self.bump();
                            lhs = self.complete(marker, SyntaxKind::FieldExpr);
                        }
                        Some(TokenKind::String | TokenKind::StrHead) if self.prefixed_string(1) => {
                            // grammar.primary.prefix-after-dot
                            let at = self.pos() + 1;
                            self.error_at(Code::QualifiedStringPrefix, at);
                            let marker = self.precede(lhs);
                            self.bump();
                            self.primary();
                            lhs = self.complete(marker, SyntaxKind::FieldExpr);
                        }
                        _ => {
                            // grammar.expr.no-numeric-member, lex.raw.*
                            let at = self.pos() + 1;
                            self.error_at(Code::SyntaxError, at);
                            let marker = self.precede(lhs);
                            self.bump();
                            if self
                                .current()
                                .is_some_and(|k| k == TokenKind::Number || k.is_keyword())
                            {
                                self.bump();
                            }
                            lhs = self.complete(marker, SyntaxKind::FieldExpr);
                        }
                    }
                }
                TokenKind::LParen if same_line => {
                    let marker = self.precede(lhs);
                    self.arguments();
                    lhs = self.complete(marker, SyntaxKind::CallExpr);
                }
                TokenKind::LBracket if same_line => {
                    let marker = self.precede(lhs);
                    self.bump();
                    self.expr(Ctx::Expr);
                    self.expect(TokenKind::RBracket);
                    lhs = self.complete(marker, SyntaxKind::IndexExpr);
                }
                TokenKind::Question if same_line => {
                    let marker = self.precede(lhs);
                    self.bump();
                    lhs = self.complete(marker, SyntaxKind::TryExpr);
                }
                TokenKind::Bang
                    if same_line
                        && self.nth_glued(0)
                        && matches!(
                            self.nth(1),
                            Some(TokenKind::LParen | TokenKind::ColonColon)
                        ) =>
                {
                    if lhs.kind() == SyntaxKind::TypeArgsExpr {
                        // grammar.primary.method-reference.no-bang
                        self.error(Code::SyntaxError);
                    }
                    let marker = self.precede(lhs);
                    self.bump();
                    lhs = self.complete(marker, SyntaxKind::SuspendExpr);
                }
                TokenKind::ColonColon if same_line => {
                    let qualifier = matches!(
                        lhs.kind(),
                        SyntaxKind::NameExpr
                            | SyntaxKind::FieldExpr
                            | SyntaxKind::PathExpr
                            | SyntaxKind::TypeArgsExpr
                            | SyntaxKind::SuspendExpr
                    );
                    if !qualifier {
                        // grammar.expr.type-arguments.unmarked
                        self.error(Code::SyntaxError);
                    }
                    let marker = self.precede(lhs);
                    self.bump();
                    if self.at(TokenKind::LBracket) {
                        self.function_type_arguments();
                        lhs = self.complete(marker, SyntaxKind::TypeArgsExpr);
                    } else {
                        self.expect_name();
                        if self.at(TokenKind::Bang)
                            && self.nth_glued(0)
                            && matches!(
                                self.nth(1),
                                Some(TokenKind::LParen | TokenKind::ColonColon)
                            )
                        {
                            lhs = self.complete(marker, SyntaxKind::PathExpr);
                            continue;
                        }
                        lhs = self.complete(marker, SyntaxKind::PathExpr);
                    }
                }
                TokenKind::LBrace if same_line => {
                    if !matches!(
                        lhs.kind(),
                        SyntaxKind::NameExpr | SyntaxKind::FieldExpr | SyntaxKind::TypeArgsExpr
                    ) {
                        if lhs.kind() == SyntaxKind::IndexExpr {
                            // grammar.expr.type-arguments.unmarked
                            self.error(Code::SyntaxError);
                        } else {
                            break;
                        }
                    }
                    let marker = self.precede(lhs);
                    self.data_body();
                    lhs = self.complete(marker, SyntaxKind::DataExpr);
                }
                _ => break,
            }
        }
        lhs
    }

    /// `::[ type | _, ... ]`.
    fn function_type_arguments(&mut self) {
        let marker = self.start();
        self.bump(); // [
        while !self.at(TokenKind::RBracket) && self.current().is_some() {
            let before = self.pos();
            if self.at(TokenKind::Placeholder) {
                self.token_node(SyntaxKind::InferType);
            } else if matches!(self.current(), Some(TokenKind::Ident | TokenKind::RawIdent))
                && self.nth(1) == Some(TokenKind::Eq)
            {
                // grammar.generic.binding.trait-type-only
                self.error(Code::SyntaxError);
                let binding = self.start();
                self.bump();
                self.bump();
                self.ty();
                self.complete(binding, SyntaxKind::AssociatedTypeBinding);
            } else {
                self.ty_argument();
            }
            if !self.eat(TokenKind::Comma) || self.pos() == before {
                break;
            }
        }
        self.expect(TokenKind::RBracket);
        self.complete(marker, SyntaxKind::TypeArgumentList);
    }

    /// `( positional, ..., name = value, ... )`.
    pub(crate) fn arguments(&mut self) {
        let marker = self.start();
        self.bump(); // (
        let mut named = false;
        while !self.at(TokenKind::RParen) && self.current().is_some() {
            let before = self.pos();
            if matches!(self.current(), Some(TokenKind::Ident | TokenKind::RawIdent))
                && self.nth(1) == Some(TokenKind::Eq)
            {
                named = true;
                let argument = self.start();
                self.bump();
                self.bump();
                self.expr(Ctx::Expr);
                self.complete(argument, SyntaxKind::NamedArgument);
            } else {
                if named {
                    // grammar.call.positional-first
                    let at = self.pos();
                    self.error_at(Code::ArgumentOrder, at);
                }
                let argument = self.start();
                self.element(Ctx::Expr);
                self.complete(argument, SyntaxKind::Argument);
            }
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RParen) {
                    self.list_error();
                    self.skip_to_list_end(TokenKind::RParen);
                    if self.eat(TokenKind::Comma) {
                        continue;
                    }
                }
                break;
            }
            if self.pos() == before {
                break;
            }
        }
        self.expect(TokenKind::RParen);
        self.complete(marker, SyntaxKind::ArgumentList);
    }

    /// An element that may end in a suffix spread `...`.
    fn element(&mut self, ctx: Ctx) -> Done {
        let value = self.expr(ctx);
        if self.at(TokenKind::Ellipsis) {
            let spread = self.precede(value);
            self.bump();
            return self.complete(spread, SyntaxKind::SpreadExpr);
        }
        value
    }

    /// A list element was not followed by `,` or the closer.
    fn list_error(&mut self) {
        if self.at(TokenKind::Colon) {
            // grammar.call.trailing-block.not-header: inside brackets.
            let at = self.pos();
            self.error_at(Code::TrailingBlockPosition, at);
        } else {
            self.error(Code::SyntaxError);
        }
    }

    // ------------------------------------------------------------ primary

    fn primary(&mut self) -> Done {
        match self.current() {
            Some(
                TokenKind::Number
                | TokenKind::Char
                | TokenKind::KwTrue
                | TokenKind::KwFalse
                | TokenKind::KwPass,
            ) => self.token_node(SyntaxKind::LiteralExpr),
            Some(TokenKind::String) => self.token_node(SyntaxKind::StringExpr),
            Some(TokenKind::StrHead) => self.interpolated_string(),
            Some(
                TokenKind::Ident
                | TokenKind::RawIdent
                | TokenKind::KwSelfValue
                | TokenKind::KwSelfType,
            ) => self.token_node(SyntaxKind::NameExpr),
            Some(TokenKind::Placeholder) => self.token_node(SyntaxKind::PlaceholderExpr),
            Some(TokenKind::Dot) => {
                let marker = self.start();
                if self.nth(1) == Some(TokenKind::Number) && self.nth_glued(1) {
                    // lex.float.no-bare-point
                    let at = self.pos();
                    self.error_at(Code::InvalidToken, at);
                    self.bump();
                    self.bump();
                    return self.complete(marker, SyntaxKind::Error);
                }
                self.bump();
                self.expect_name();
                self.complete(marker, SyntaxKind::VariantExpr)
            }
            Some(TokenKind::LParen) => self.paren(),
            Some(TokenKind::LBracket) => self.list(),
            Some(TokenKind::LBrace) => self.map(),
            Some(TokenKind::Dollar) => self.context_expr(),
            Some(
                TokenKind::KwIf
                | TokenKind::KwFor
                | TokenKind::KwWhile
                | TokenKind::KwMatch
                | TokenKind::KwFn,
            ) => {
                // A suite expression as an operand needs parentheses.
                self.error(Code::SyntaxError);
                if self.suite_start() {
                    self.suite_expr(Ctx::Closed)
                } else {
                    let marker = self.start();
                    self.bump();
                    self.complete(marker, SyntaxKind::Error)
                }
            }
            _ => {
                let marker = self.start();
                self.error(Code::SyntaxError);
                if self.current().is_some_and(|kind| {
                    !kind.closes()
                        && !matches!(
                            kind,
                            TokenKind::Comma
                                | TokenKind::Colon
                                | TokenKind::StrMid
                                | TokenKind::FatArrow
                        )
                }) {
                    self.bump();
                }
                self.complete(marker, SyntaxKind::Error)
            }
        }
    }

    fn interpolated_string(&mut self) -> Done {
        let marker = self.start();
        self.bump(); // head
        loop {
            let part = self.start();
            if !self.at(TokenKind::StrMid) && !self.at(TokenKind::StrTail) {
                self.expr(Ctx::Expr);
            }
            if !matches!(self.current(), Some(TokenKind::StrMid | TokenKind::StrTail)) {
                self.error(Code::SyntaxError);
                self.skip_balanced();
            }
            self.complete(part, SyntaxKind::Interpolation);
            if self.eat(TokenKind::StrMid) {
                continue;
            }
            self.eat(TokenKind::StrTail);
            break;
        }
        self.complete(marker, SyntaxKind::StringExpr)
    }

    /// `()`, `(e)`, `(e, ...)`, with a spread last.
    fn paren(&mut self) -> Done {
        let marker = self.start();
        self.bump(); // (
        if self.eat(TokenKind::RParen) {
            return self.complete(marker, SyntaxKind::TupleExpr);
        }
        let mut count = 0;
        let mut comma = false;
        let mut spread_seen = false;
        while !self.at(TokenKind::RParen) && self.current().is_some() {
            let before = self.pos();
            if spread_seen {
                // grammar.primary.tuple-spread
                self.error(Code::SyntaxError);
            }
            let element = self.element(Ctx::Expr);
            spread_seen |= element.kind() == SyntaxKind::SpreadExpr;
            count += 1;
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RParen) {
                    self.list_error();
                    self.skip_to_list_end(TokenKind::RParen);
                    if self.eat(TokenKind::Comma) {
                        comma = true;
                        continue;
                    }
                }
                break;
            }
            comma = true;
            if self.pos() == before {
                break;
            }
        }
        if spread_seen && !comma {
            self.error(Code::SyntaxError);
        }
        self.expect(TokenKind::RParen);
        let kind = if count == 1 && !comma {
            SyntaxKind::ParenExpr
        } else {
            SyntaxKind::TupleExpr
        };
        self.complete(marker, kind)
    }

    fn list(&mut self) -> Done {
        let marker = self.start();
        self.bump(); // [
        if self.at(TokenKind::KwFor) && self.comprehension_ahead() {
            self.comprehension_clauses();
            if self.expect(TokenKind::FatArrow) {
                self.expr(Ctx::Expr);
            }
            self.expect(TokenKind::RBracket);
            return self.complete(marker, SyntaxKind::ComprehensionExpr);
        }
        while !self.at(TokenKind::RBracket) && self.current().is_some() {
            let before = self.pos();
            self.element(Ctx::Expr);
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RBracket) {
                    self.list_error();
                    self.skip_to_list_end(TokenKind::RBracket);
                    if self.eat(TokenKind::Comma) {
                        continue;
                    }
                }
                break;
            }
            if self.pos() == before {
                break;
            }
        }
        self.expect(TokenKind::RBracket);
        self.complete(marker, SyntaxKind::ListExpr)
    }

    fn map(&mut self) -> Done {
        let marker = self.start();
        self.bump(); // {
        if self.at(TokenKind::KwFor) {
            self.comprehension_clauses();
            if self.expect(TokenKind::FatArrow) {
                let entry = self.start();
                self.expr(Ctx::Continued);
                if self.expect(TokenKind::Colon) {
                    self.expr(Ctx::Expr);
                }
                self.complete(entry, SyntaxKind::MapEntry);
            }
            self.expect(TokenKind::RBrace);
            return self.complete(marker, SyntaxKind::ComprehensionExpr);
        }
        while !self.at(TokenKind::RBrace) && self.current().is_some() {
            let before = self.pos();
            let entry = self.start();
            self.expr(Ctx::Continued);
            if self.expect(TokenKind::Colon) {
                self.expr(Ctx::Expr);
            }
            self.complete(entry, SyntaxKind::MapEntry);
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RBrace) {
                    self.error(Code::SyntaxError);
                    self.skip_to_list_end(TokenKind::RBrace);
                    if self.eat(TokenKind::Comma) {
                        continue;
                    }
                }
                break;
            }
            if self.pos() == before {
                break;
            }
        }
        self.expect(TokenKind::RBrace);
        self.complete(marker, SyntaxKind::MapExpr)
    }

    /// After `[for`: a `=>` before any `:` at this depth makes a
    /// comprehension; a `:` makes a `for` loop element.
    fn comprehension_ahead(&mut self) -> bool {
        let mut depth = 0_u32;
        for &kind in &self.toks.kind[self.pos()..] {
            match kind {
                TokenKind::FatArrow if depth == 0 => return true,
                TokenKind::Colon if depth == 0 => return false,
                kind if kind.opens() => depth += 1,
                kind if kind.closes() => {
                    if depth == 0 {
                        return true;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        true
    }

    fn comprehension_clauses(&mut self) {
        loop {
            if self.at(TokenKind::KwFor) {
                let clause = self.start();
                self.bump();
                self.pattern(false);
                if self.expect(TokenKind::KwIn) {
                    self.expr(Ctx::Continued);
                }
                self.complete(clause, SyntaxKind::ComprehensionFor);
            } else if self.at(TokenKind::KwIf) {
                let clause = self.start();
                self.bump();
                self.expr(Ctx::Continued);
                self.complete(clause, SyntaxKind::ComprehensionIf);
            } else {
                break;
            }
        }
    }

    /// `{ ...base, field: value, shorthand }` after a type name.
    fn data_body(&mut self) {
        self.bump(); // {
        let mut first = true;
        while !self.at(TokenKind::RBrace) && self.current().is_some() {
            let before = self.pos();
            if self.at(TokenKind::Ellipsis) {
                if !first {
                    // grammar: the copy-update spread comes first.
                    self.error(Code::SyntaxError);
                }
                let spread = self.start();
                self.bump();
                self.expr(Ctx::Expr);
                self.complete(spread, SyntaxKind::SpreadExpr);
            } else {
                let field = self.start();
                self.expect_name();
                if self.eat(TokenKind::Colon) {
                    self.eat(TokenKind::Ellipsis);
                    self.expr(Ctx::Expr);
                }
                self.complete(field, SyntaxKind::DataFieldInit);
            }
            first = false;
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RBrace) {
                    self.error(Code::SyntaxError);
                    self.skip_to_list_end(TokenKind::RBrace);
                    if self.eat(TokenKind::Comma) {
                        continue;
                    }
                }
                break;
            }
            if self.pos() == before {
                break;
            }
        }
        self.expect(TokenKind::RBrace);
    }
}
