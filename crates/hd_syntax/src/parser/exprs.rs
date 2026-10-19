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
                    | TokenKind::Amp
                    | TokenKind::Star
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

    /// `name := expr`, the name being the current token.
    fn binding_expr(&mut self, ctx: Ctx) -> Done {
        let marker = self.start();
        self.token_node(SyntaxKind::BindingPattern);
        self.bump(); // :=
        let inner = if ctx == Ctx::Rhs { Ctx::Closed } else { ctx };
        self.expr(inner);
        self.complete(marker, SyntaxKind::BindingExpr)
    }

    fn expr_inner(&mut self, ctx: Ctx) -> Done {
        if matches!(self.current(), Some(TokenKind::Ident | TokenKind::RawIdent))
            && self.nth(1) == Some(TokenKind::ColonEq)
        {
            return self.binding_expr(ctx);
        }
        if let Some(kind) = self.current()
            && kind.is_keyword()
            && self.nth(1) == Some(TokenKind::ColonEq)
        {
            // grammar.tests.keyword: a reserved word never names a binding.
            let word = self.nth_text(0);
            self.error(
                Code::SyntaxError,
                &format!("'{word}' is a reserved word, so it cannot name a binding"),
            );
            self.bound_keywords.push(kind);
            return self.binding_expr(ctx);
        }
        if self.suite_start() {
            let bad = match ctx {
                Ctx::Closed => true,
                Ctx::Inline => matches!(self.current(), Some(TokenKind::KwIf | TokenKind::KwMatch)),
                _ => false,
            };
            if bad {
                // grammar.closed.*, grammar.inline.no-if
                let message = if ctx == Ctx::Closed {
                    "an expression followed by another token outside brackets cannot end in a suite"
                        .to_owned()
                } else {
                    format!(
                        "a same-line suite cannot contain '{}'; put the body on an indented line",
                        self.nth_text(0)
                    )
                };
                self.error(Code::SyntaxError, &message);
            }
            let done = self.suite_expr(ctx);
            if ctx == Ctx::Continued && self.suite_inline {
                // grammar.continued.no-same-line-suite
                self.error(Code::SyntaxError, "a same-line suite cannot end an expression that is followed by another token inside brackets");
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
            self.expected("a line ending");
            return callee;
        }
        if !self.nth_line_first(1) && self.nth(1).is_some() {
            // grammar.call.trailing-block.next-line, grammar.stmt.typed-binding.let
            if ctx == Ctx::Stmt && callee.kind() == SyntaxKind::NameExpr {
                let error = self.errors.len();
                self.error(
                    Code::MissingLet,
                    "a binding with a type annotation must begin with 'let'; write `let x: T = ...`, or `x := ...` without the type",
                );
                let at = self.pos();
                let line = self.toks.line_of(self.toks.start[at]);
                let end = self
                    .toks
                    .line_start
                    .get(line + 1)
                    .copied()
                    .unwrap_or(u32::MAX);
                let initialized = (at + 1..self.toks.len())
                    .take_while(|&i| self.toks.start[i] < end)
                    .any(|i| self.toks.kind[i] == TokenKind::Eq);
                if initialized {
                    let (lo, _) = self.token_span(at.saturating_sub(1));
                    self.fix_error(
                        error,
                        "insert `let` before the binding",
                        &[(lo, lo, "let ")],
                    );
                }
            } else {
                let found = self.toks.describe(self.pos() + 1, self.src);
                let message = format!("expected a line ending after ':', found {found}");
                self.error(Code::SyntaxError, &message);
            }
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

    /// `flow.if.else-if`: the chain is one `IfExpr` whose branches are
    /// siblings, so a branch adds no nesting level
    /// (`grammar.limit.nesting`) and the parser loops rather than recurses.
    fn if_expr(&mut self, ctx: Ctx) -> Done {
        let marker = self.start();
        self.bump(); // if
        self.expr(ctx.header());
        if self.expect(TokenKind::Colon, "':'") {
            self.suite(false, Ctx::Inline);
            while self.at_else() {
                let clause = self.start();
                self.bump(); // else
                if !self.eat(TokenKind::KwIf) {
                    if self.expect(TokenKind::Colon, "':'") {
                        self.suite(false, Ctx::Inline);
                    }
                    self.complete(clause, SyntaxKind::ElseClause);
                    break;
                }
                self.expr(ctx.header());
                let closed = self.expect(TokenKind::Colon, "':'");
                if closed {
                    self.suite(false, Ctx::Inline);
                }
                self.complete(clause, SyntaxKind::ElseIfClause);
                if !closed {
                    break;
                }
            }
        }
        self.complete(marker, SyntaxKind::IfExpr)
    }

    fn loop_else(&mut self) {
        if self.at_else() {
            let clause = self.start();
            self.bump();
            if self.expect(TokenKind::Colon, "':'") {
                self.suite(false, Ctx::Inline);
            }
            self.complete(clause, SyntaxKind::ElseClause);
        }
    }

    fn for_expr(&mut self, ctx: Ctx) -> Done {
        let marker = self.start();
        self.bump(); // for
        self.pattern(false);
        if self.expect(TokenKind::KwIn, "'in'") {
            self.expr(ctx.header());
            if self.expect(TokenKind::Colon, "':'") {
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
        if self.expect(TokenKind::Colon, "':'") {
            self.suite(false, Ctx::Inline);
            self.loop_else();
        }
        self.complete(marker, SyntaxKind::WhileExpr)
    }

    fn match_expr(&mut self, ctx: Ctx) -> Done {
        let marker = self.start();
        self.bump(); // match
        self.expr(ctx.header());
        if self.expect(TokenKind::Colon, "':'") {
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
                    self.expected("a line ending after the match header");
                    self.recover_line();
                }
                SuiteKind::Missing => self.error(
                    Code::SyntaxError,
                    "expected indented match arms, found a line that is not indented deeper",
                ),
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
        if self.at(TokenKind::Pipe) {
            self.error(
                Code::SyntaxError,
                "match patterns do not use '|'; write a separate arm for each pattern",
            );
        }
        if self.expect(TokenKind::FatArrow, "'=>'") {
            let body = self.start();
            match self.cur.open_arm() {
                SuiteKind::Indented => {
                    self.indented_body();
                    self.suite_closed = true;
                    self.suite_inline = false;
                }
                SuiteKind::SameLine => self.simple_statement(Ctx::Stmt),
                SuiteKind::Missing => self.error(
                    Code::SyntaxError,
                    "expected an indented match arm body, found a line that is not indented deeper",
                ),
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
        if self.expect(TokenKind::Colon, "':'") {
            self.suite(true, Ctx::Inline);
        }
        self.complete(marker, SyntaxKind::ClosureExpr)
    }

    /// `$.use(...)`, `$.context(...)`, `$.with(...): suite`, `$.Context[...]`.
    fn context_expr(&mut self) -> Done {
        let marker = self.start();
        self.bump(); // $
        if !self.expect(TokenKind::Dot, "'.'") {
            return self.complete(marker, SyntaxKind::ContextExpr);
        }
        let word = self.nth_text(0);
        if !self.at(TokenKind::Ident) {
            self.expected("a provider-context operation after '$.'");
            return self.complete(marker, SyntaxKind::ContextExpr);
        }
        let operation = self.pos();
        self.bump();
        match word {
            "use" => {
                if self.expect(TokenKind::LParen, "'('") {
                    while !self.at(TokenKind::RParen) && self.current().is_some() {
                        let before = self.pos();
                        self.ty_bound_trait(true);
                        if !self.eat(TokenKind::Comma) || self.pos() == before {
                            break;
                        }
                    }
                    self.expect(TokenKind::RParen, "')'");
                }
            }
            "context" | "with" => {
                if self.at(TokenKind::LParen) {
                    self.context_entries();
                } else {
                    self.expected("'(' after the context operation");
                }
                if word == "with" && self.expect(TokenKind::Colon, "':'") {
                    self.suite(false, Ctx::Inline);
                }
            }
            "Context" => {
                if self.expect(TokenKind::LBracket, "'['") {
                    self.ty_argument();
                    self.expect(TokenKind::RBracket, "']'");
                }
            }
            _ => self.error_at(Code::SyntaxError, operation, &format!("unknown provider-context operation '$.{word}'; expected 'use', 'context', 'with', or 'Context'")),
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
                if self.expect(TokenKind::Eq, "'='") {
                    self.expr(Ctx::Expr);
                }
            } else {
                if self.at(TokenKind::Ellipsis) {
                    // grammar.primary.prefix-elsewhere
                    self.error(
                        Code::SyntaxError,
                        "a spread is written after its value: write 'value...'",
                    );
                    self.bump();
                }
                let value = self.expr(Ctx::Continued);
                if self.at(TokenKind::Ellipsis) {
                    let spread = self.precede(value);
                    self.bump();
                    self.complete(spread, SyntaxKind::SpreadExpr);
                } else {
                    self.error(
                        Code::SyntaxError,
                        "a provider entry is 'Key = value' or a spread 'value...'",
                    );
                }
            }
            self.complete(entry, SyntaxKind::ContextEntry);
            if !self.eat(TokenKind::Comma) || self.pos() == before {
                break;
            }
        }
        self.expect(TokenKind::RParen, "')'");
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

    /// `grammar.expr.range.inclusive-needs-end`, reported at the `..=`:
    /// what follows it may already be the next line.
    fn missing_range_end(&mut self, operator: usize) {
        self.error_at(
            Code::SyntaxError,
            operator,
            "an inclusive range needs an end bound: write 'a..=b'",
        );
    }

    fn range_expr(&mut self) -> Done {
        if matches!(
            self.current(),
            Some(TokenKind::DotDot | TokenKind::DotDotEq)
        ) {
            let marker = self.start();
            let operator = self.pos();
            let inclusive = self.at(TokenKind::DotDotEq);
            self.bump();
            if self.can_begin_operand() {
                self.binary(0);
            } else if inclusive {
                self.missing_range_end(operator);
            }
            return self.complete(marker, SyntaxKind::RangeExpr);
        }
        let lhs = self.binary(0);
        if matches!(
            self.current(),
            Some(TokenKind::DotDot | TokenKind::DotDotEq)
        ) {
            let marker = self.precede(lhs);
            let operator = self.pos();
            let inclusive = self.at(TokenKind::DotDotEq);
            self.bump();
            if self.can_begin_operand() {
                self.binary(0);
            } else if inclusive {
                self.missing_range_end(operator);
            }
            let done = self.complete(marker, SyntaxKind::RangeExpr);
            if matches!(
                self.current(),
                Some(TokenKind::DotDot | TokenKind::DotDotEq)
            ) {
                // grammar.expr.range.non-assoc
                self.error(
                    Code::SyntaxError,
                    "ranges do not chain: parenthesize a range used as a bound",
                );
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
                self.error_at(Code::ComparisonChaining, at, "comparisons do not chain");
            }
        }
        lhs
    }

    fn unary(&mut self) -> Done {
        if self.at(TokenKind::Star) {
            // ref.prec.deref: `*` binds like a postfix operator, tighter than `**`
            let deref = self.deref();
            return self.power_tail(deref);
        }
        if matches!(
            self.current(),
            Some(
                TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Bang
                    | TokenKind::Tilde
                    | TokenKind::Amp
            )
        ) {
            if !self.enter() {
                let marker = self.start();
                self.skip_balanced();
                return self.complete(marker, SyntaxKind::Error);
            }
            let marker = self.start();
            let kind = if self.at(TokenKind::Amp) {
                // ref.expr.make
                self.bump();
                self.eat(TokenKind::KwMut);
                SyntaxKind::RefExpr
            } else {
                self.bump();
                SyntaxKind::UnaryExpr
            };
            self.unary();
            self.leave();
            return self.complete(marker, kind);
        }
        let base = self.postfix();
        self.power_tail(base)
    }

    /// `**` after an operand, and its right operand.
    fn power_tail(&mut self, base: Done) -> Done {
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

    /// `*e` (ref.expr.deref-operand): the operand is a postfix expression, another
    /// `*e`, or another prefix operator with its own operand.
    fn deref(&mut self) -> Done {
        if !self.enter() {
            let marker = self.start();
            self.skip_balanced();
            return self.complete(marker, SyntaxKind::Error);
        }
        let marker = self.start();
        self.bump();
        if self.at(TokenKind::Star) {
            self.deref();
        } else if matches!(
            self.current(),
            Some(
                TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Bang
                    | TokenKind::Tilde
                    | TokenKind::Amp
            )
        ) {
            self.unary();
        } else {
            self.postfix();
        }
        self.leave();
        self.complete(marker, SyntaxKind::DerefExpr)
    }

    fn postfix(&mut self) -> Done {
        let first = self.pos();
        let start = self.toks.start.get(first).copied().unwrap_or(0) as usize;
        let mut callee_end = start;
        let mut callee_bang = start;
        let mut type_arguments_start = start;
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
                            self.error_at(Code::QualifiedStringPrefix, at, "a string prefix is a bare name in scope: write 'tag\"...\"' without the qualifier");
                            let marker = self.precede(lhs);
                            self.bump();
                            self.primary();
                            lhs = self.complete(marker, SyntaxKind::FieldExpr);
                        }
                        _ => {
                            // grammar.expr.no-numeric-member, lex.raw.*
                            let at = self.pos() + 1;
                            self.expected_at("a member name after '.'", at);
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
                    callee_end = self.toks.start[self.pos()] as usize;
                    callee_bang = if lhs.kind() == SyntaxKind::TypeArgsExpr {
                        type_arguments_start
                    } else {
                        callee_end
                    };
                    let marker = self.precede(lhs);
                    self.arguments();
                    lhs = self.complete(marker, SyntaxKind::CallExpr);
                }
                TokenKind::LBracket if same_line => {
                    let marker = self.precede(lhs);
                    self.bump();
                    self.expr(Ctx::Expr);
                    self.expect(TokenKind::RBracket, "']'");
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
                        self.error(Code::SyntaxError, "a bang call puts '!' before the type arguments: write 'name!::[T](...)'");
                    }
                    let marker = self.precede(lhs);
                    self.bump();
                    lhs = self.complete(marker, SyntaxKind::SuspendExpr);
                }
                TokenKind::Bang
                    if same_line && self.nth_glued(0) && lhs.kind() == SyntaxKind::CallExpr =>
                {
                    let prefix = self.src[start..callee_bang].trim_end();
                    let suffix = self.src[callee_bang..callee_end].trim_end();
                    let message = format!(
                        "a bang call puts '!' before the arguments: write '{prefix}!{suffix}(...)'"
                    );
                    self.error(Code::SyntaxError, &message);
                    break;
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
                        self.error(
                            Code::SyntaxError,
                            "'::' follows a name or a path, not another expression",
                        );
                    }
                    let marker = self.precede(lhs);
                    let qualifier_start = self.toks.start[self.pos()] as usize;
                    self.bump();
                    if self.at(TokenKind::LBracket) {
                        type_arguments_start = qualifier_start;
                        self.function_type_arguments();
                        lhs = self.complete(marker, SyntaxKind::TypeArgsExpr);
                    } else {
                        self.expect_name("an associated function name");
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
                            self.error(Code::SyntaxError, "type arguments in an expression follow '::', as in 'Box::[i32] { ... }'");
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
                self.error(
                    Code::SyntaxError,
                    "a type-argument list in an expression takes no 'Name = type' binding",
                );
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
        self.expect(TokenKind::RBracket, "']'");
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
                    self.error_at(
                        Code::ArgumentOrder,
                        at,
                        "positional arguments must precede named arguments",
                    );
                }
                let argument = self.start();
                self.element(Ctx::Expr);
                self.complete(argument, SyntaxKind::Argument);
            }
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RParen) {
                    self.list_error(')');
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
        self.expect(TokenKind::RParen, "')'");
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
    fn list_error(&mut self, closer: char) {
        if self.at(TokenKind::Colon) {
            // grammar.call.trailing-block.not-header: inside brackets.
            let at = self.pos();
            self.error_at(
                Code::TrailingBlockPosition,
                at,
                "a trailing callback block is not valid inside brackets",
            );
        } else {
            self.expected(&format!("',' or '{closer}'"));
        }
    }

    // ------------------------------------------------------------ primary

    fn primary(&mut self) -> Done {
        if self
            .current()
            .is_some_and(|kind| self.bound_keywords.contains(&kind))
        {
            return self.token_node(SyntaxKind::NameExpr);
        }
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
                    self.error_at(
                        Code::InvalidToken,
                        at,
                        "a number needs a digit before '.': write '0.5'",
                    );
                    self.bump();
                    self.bump();
                    return self.complete(marker, SyntaxKind::Error);
                }
                self.bump();
                self.expect_name("an enum variant name after '.'");
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
                let message = format!(
                    "a '{}' expression used as an operand needs parentheses",
                    self.nth_text(0)
                );
                self.error(Code::SyntaxError, &message);
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
                if self.at(TokenKind::Slash)
                    && self.nth_line_first(0)
                    && self.nth(1) == Some(TokenKind::Slash)
                    && self.nth(2) == Some(TokenKind::Slash)
                    && self.nth_glued(1)
                    && self.nth_glued(2)
                {
                    self.error(
                        Code::SyntaxError,
                        "documentation comments start with '##', not '///'",
                    );
                } else {
                    self.expected("an expression");
                }
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
                self.expected("the end of an interpolation expression");
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
                self.error(
                    Code::SyntaxError,
                    "a spread must be the last element of a tuple",
                );
            }
            let element = self.element(Ctx::Expr);
            spread_seen |= element.kind() == SyntaxKind::SpreadExpr;
            count += 1;
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RParen) {
                    if self.at(TokenKind::Colon) && count == 1 && !comma {
                        let message = self.cast_message(before);
                        self.error(Code::TrailingBlockPosition, &message);
                    }
                    self.list_error(')');
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
            self.error(
                Code::SyntaxError,
                "a lone spread in a tuple keeps its trailing comma: write '(xs...,)'",
            );
        }
        self.expect(TokenKind::RParen, "')'");
        let kind = if count == 1 && !comma {
            SyntaxKind::ParenExpr
        } else {
            SyntaxKind::TupleExpr
        };
        self.complete(marker, kind)
    }

    /// Suggest the written conversion for one short operand token or a
    /// signed numeric literal. Larger expressions use a schematic hint.
    fn cast_message(&mut self, operand: usize) -> String {
        let fallback = "casts are calls: write 'Type(value)'";
        let colon = self.pos();
        let signed_number = colon == operand + 2
            && matches!(self.toks.kind[operand], TokenKind::Plus | TokenKind::Minus)
            && self.toks.kind[operand + 1] == TokenKind::Number;
        if colon != operand + 1 && !signed_number {
            return fallback.into();
        }
        let value = &self.src[self.toks.start[operand] as usize..self.toks.end[colon - 1] as usize];
        if value.len() > 32 || value.contains(['\n', '\r']) {
            return fallback.into();
        }
        let mut depth = 0_usize;
        for end in colon + 1..self.toks.len() {
            let kind = self.toks.kind[end];
            if depth == 0 && kind == TokenKind::RParen {
                let ty =
                    self.src[self.toks.end[colon] as usize..self.toks.start[end] as usize].trim();
                return if ty.is_empty() || ty.contains(['\n', '\r']) {
                    fallback.into()
                } else {
                    format!("casts are calls: write '{ty}({value})'")
                };
            }
            if kind.opens() {
                depth += 1;
            } else if kind.closes() {
                let Some(outer) = depth.checked_sub(1) else {
                    break;
                };
                depth = outer;
            } else if depth == 0 && kind == TokenKind::Comma {
                break;
            }
        }
        fallback.into()
    }

    fn list(&mut self) -> Done {
        let marker = self.start();
        self.bump(); // [
        if self.at(TokenKind::KwFor) && self.comprehension_ahead() {
            self.comprehension_clauses();
            if self.expect(TokenKind::FatArrow, "'=>'") {
                self.expr(Ctx::Expr);
            }
            self.expect(TokenKind::RBracket, "']'");
            return self.complete(marker, SyntaxKind::ComprehensionExpr);
        }
        while !self.at(TokenKind::RBracket) && self.current().is_some() {
            let before = self.pos();
            self.element(Ctx::Expr);
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RBracket) {
                    self.list_error(']');
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
        self.expect(TokenKind::RBracket, "']'");
        self.complete(marker, SyntaxKind::ListExpr)
    }

    fn map(&mut self) -> Done {
        let marker = self.start();
        self.bump(); // {
        if self.at(TokenKind::KwFor) {
            self.comprehension_clauses();
            if self.expect(TokenKind::FatArrow, "'=>'") {
                let entry = self.start();
                self.expr(Ctx::Continued);
                if self.expect(TokenKind::Colon, "':'") {
                    self.expr(Ctx::Expr);
                }
                self.complete(entry, SyntaxKind::MapEntry);
            }
            self.expect(TokenKind::RBrace, "'}'");
            return self.complete(marker, SyntaxKind::ComprehensionExpr);
        }
        while !self.at(TokenKind::RBrace) && self.current().is_some() {
            let before = self.pos();
            let entry = self.start();
            self.expr(Ctx::Continued);
            if self.expect(TokenKind::Colon, "':'") {
                self.expr(Ctx::Expr);
            }
            self.complete(entry, SyntaxKind::MapEntry);
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RBrace) {
                    self.expected("',' or '}'");
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
        self.expect(TokenKind::RBrace, "'}'");
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
                if self.expect(TokenKind::KwIn, "'in'") {
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
                    self.error(
                        Code::SyntaxError,
                        "the copy-update spread '...base' comes first in a data expression",
                    );
                }
                let spread = self.start();
                self.bump();
                self.expr(Ctx::Expr);
                self.complete(spread, SyntaxKind::SpreadExpr);
            } else {
                let field = self.start();
                self.expect_name("a data field name");
                if self.eat(TokenKind::Colon) {
                    self.eat(TokenKind::Ellipsis);
                    self.expr(Ctx::Expr);
                }
                self.complete(field, SyntaxKind::DataFieldInit);
            }
            first = false;
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RBrace) {
                    self.expected("',' or '}'");
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
        self.expect(TokenKind::RBrace, "'}'");
    }
}
