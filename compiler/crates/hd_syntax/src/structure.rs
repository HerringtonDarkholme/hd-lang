//! Structure inside one logical line (syntax.md §4.4): function and impl
//! headers, data fields, statements and expressions by precedence
//! climbing. The line framework in `parser.rs` owns lines, blocks and
//! recovery between lines; this module only nests the line's tokens.
//!
//! It never reports a diagnostic and never fails: tokens it cannot place
//! go into an `Error` node, and every loop consumes a token or stops, so
//! the event vector stays balanced on any input.

use hd_base::TokenIdx;

use crate::green::Event;
use crate::{SyntaxKind, TokenBuf, TokenKind};

pub(crate) struct Line<'e> {
    kinds: &'e [TokenKind],
    pos: usize,
    end: usize,
    events: &'e mut Vec<Event>,
}

fn as_u32(v: usize) -> u32 {
    u32::try_from(v).expect("token index")
}

impl<'e> Line<'e> {
    pub(crate) fn new(
        tokens: &'e TokenBuf,
        first: usize,
        end: usize,
        events: &'e mut Vec<Event>,
    ) -> Self {
        Self {
            kinds: &tokens.kind,
            pos: first,
            end,
            events,
        }
    }

    fn peek(&self) -> Option<TokenKind> {
        (self.pos < self.end).then(|| self.kinds[self.pos])
    }
    fn nth(&self, n: usize) -> Option<TokenKind> {
        (self.pos + n < self.end).then(|| self.kinds[self.pos + n])
    }
    fn at(&self, k: TokenKind) -> bool {
        self.peek() == Some(k)
    }
    fn bump(&mut self) {
        if self.pos < self.end {
            self.events
                .push(Event::Token(TokenIdx::from_raw(as_u32(self.pos))));
            self.pos += 1;
        }
    }
    fn eat(&mut self, k: TokenKind) -> bool {
        if self.at(k) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn start(&mut self, k: SyntaxKind) {
        self.events.push(Event::Start(k));
    }
    fn finish(&mut self) {
        self.events.push(Event::Finish);
    }
    fn mark(&self) -> usize {
        self.events.len()
    }
    fn wrap(&mut self, mark: usize, k: SyntaxKind) {
        self.events.insert(mark, Event::Start(k));
    }
    /// The rest of the line, as one `Error` node when anything is left.
    fn rest(&mut self) {
        if self.pos < self.end {
            self.start(SyntaxKind::Error);
            while self.pos < self.end {
                self.bump();
            }
            self.finish();
        }
    }
    /// One token in an `Error` node, so a loop always progresses.
    fn junk(&mut self) {
        self.start(SyntaxKind::Error);
        self.bump();
        self.finish();
    }

    // ------------------------------------------------------------ headers

    /// `[pub] fn NAME [GENERICS] (PARAMS) [-> TYPE] [:]`.
    pub(crate) fn fn_header(&mut self) {
        self.eat(TokenKind::KwPub);
        self.eat(TokenKind::KwFn);
        if !self.eat(TokenKind::Ident) {
            self.eat(TokenKind::RawIdent);
        }
        if self.at(TokenKind::LBracket) {
            self.start(SyntaxKind::GenericParameterList);
            self.bump();
            while !self.at(TokenKind::RBracket) && self.peek().is_some() {
                self.start(SyntaxKind::GenericParameter);
                if !self.eat(TokenKind::Ident) {
                    self.junk();
                }
                if self.eat(TokenKind::Lt) {
                    self.ty();
                }
                self.finish();
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            self.eat(TokenKind::RBracket);
            self.finish();
        }
        if self.at(TokenKind::LParen) {
            self.start(SyntaxKind::ParameterList);
            self.bump();
            while !self.at(TokenKind::RParen) && self.peek().is_some() {
                self.start(SyntaxKind::Parameter);
                if !self.eat(TokenKind::KwSelfValue) {
                    self.eat(TokenKind::KwMut);
                    if self.eat(TokenKind::Ident) && self.eat(TokenKind::Colon) {
                        self.ty();
                    } else {
                        self.skip_to_separator();
                    }
                }
                self.finish();
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            if !self.eat(TokenKind::RParen) {
                self.skip_to_separator();
                self.eat(TokenKind::RParen);
            }
            self.finish();
        }
        if self.eat(TokenKind::Arrow) {
            self.ty();
        }
        self.eat(TokenKind::Colon);
        self.rest();
    }

    /// `impl TYPE [for TYPE] [:]`.
    pub(crate) fn impl_header(&mut self) {
        self.eat(TokenKind::KwPub);
        self.eat(TokenKind::KwImpl);
        self.ty();
        if self.eat(TokenKind::KwFor) {
            self.ty();
        }
        self.eat(TokenKind::Colon);
        self.rest();
    }

    /// `[pub] NAME: TYPE`.
    pub(crate) fn data_field(&mut self) {
        self.eat(TokenKind::KwPub);
        self.eat(TokenKind::KwMut);
        if self.eat(TokenKind::Ident) && self.eat(TokenKind::Colon) {
            self.ty();
        }
        self.rest();
    }

    /// Skips to `,` `)` `]` or the end, as one `Error` node.
    fn skip_to_separator(&mut self) {
        if matches!(
            self.peek(),
            None | Some(TokenKind::Comma | TokenKind::RParen | TokenKind::RBracket)
        ) {
            return;
        }
        self.start(SyntaxKind::Error);
        let mut depth = 0u32;
        while let Some(k) = self.peek() {
            if depth == 0
                && matches!(
                    k,
                    TokenKind::Comma | TokenKind::RParen | TokenKind::RBracket
                )
            {
                break;
            }
            if k.is_open_delimiter() {
                depth += 1;
            } else if k.is_close_delimiter() {
                depth = depth.saturating_sub(1);
            }
            self.bump();
        }
        self.finish();
    }

    /// A named type with optional `[ARGS]`; anything else is an `Error`
    /// node up to the next separator.
    fn ty(&mut self) {
        match self.peek() {
            Some(TokenKind::KwDyn) => {
                self.start(SyntaxKind::DynType);
                self.bump();
                self.ty();
                self.finish();
            }
            Some(TokenKind::Ident | TokenKind::KwSelfType) => {
                self.start(SyntaxKind::NamedType);
                self.bump();
                while self.at(TokenKind::Dot) && self.nth(1) == Some(TokenKind::Ident) {
                    self.bump();
                    self.bump();
                }
                if self.at(TokenKind::LBracket) {
                    self.start(SyntaxKind::TypeArgumentList);
                    self.bump();
                    while !self.at(TokenKind::RBracket) && self.peek().is_some() {
                        self.ty();
                        if !self.eat(TokenKind::Comma) {
                            break;
                        }
                    }
                    if !self.eat(TokenKind::RBracket) {
                        self.skip_to_separator();
                        self.eat(TokenKind::RBracket);
                    }
                    self.finish();
                }
                self.finish();
            }
            _ => self.skip_to_separator(),
        }
    }

    // --------------------------------------------------------- statements

    /// One statement line; the caller has opened the line's node.
    pub(crate) fn statement(&mut self) {
        match self.peek() {
            Some(TokenKind::KwReturn) => {
                self.start(SyntaxKind::ReturnStmt);
                self.bump();
                if self.peek().is_some() {
                    self.expr(0);
                }
                self.finish();
            }
            Some(TokenKind::KwIf) => self.if_head(),
            Some(TokenKind::KwElse) => {
                self.bump();
                if self.at(TokenKind::KwIf) {
                    self.if_head();
                } else {
                    self.eat(TokenKind::Colon);
                }
            }
            Some(TokenKind::KwWhile) => {
                self.start(SyntaxKind::WhileExpr);
                self.bump();
                self.expr(0);
                self.eat(TokenKind::Colon);
                self.finish();
            }
            Some(TokenKind::KwBreak) => {
                self.start(SyntaxKind::BreakStmt);
                self.bump();
                self.finish();
            }
            Some(TokenKind::KwContinue) => {
                self.start(SyntaxKind::ContinueStmt);
                self.bump();
                self.finish();
            }
            Some(TokenKind::Ident) if self.nth(1) == Some(TokenKind::ColonEq) => {
                self.start(SyntaxKind::LetStmt);
                self.start(SyntaxKind::BindingPattern);
                self.bump();
                self.finish();
                self.bump();
                self.expr(0);
                self.finish();
            }
            _ => {
                let m = self.mark();
                self.expr(0);
                if self.eat(TokenKind::Eq) {
                    self.wrap(m, SyntaxKind::AssignmentStmt);
                    self.expr(0);
                    self.finish();
                } else {
                    self.wrap(m, SyntaxKind::ExprStmt);
                    self.finish();
                }
            }
        }
        self.rest();
    }

    fn if_head(&mut self) {
        self.start(SyntaxKind::IfExpr);
        self.bump();
        self.expr(0);
        self.eat(TokenKind::Colon);
        self.finish();
    }

    // -------------------------------------------------------- expressions

    fn binop(k: TokenKind) -> Option<(u8, SyntaxKind)> {
        Some(match k {
            TokenKind::OrOr => (1, SyntaxKind::LogicalOrExpr),
            TokenKind::AndAnd => (2, SyntaxKind::LogicalAndExpr),
            TokenKind::EqEq
            | TokenKind::NotEq
            | TokenKind::Lt
            | TokenKind::LtEq
            | TokenKind::Gt
            | TokenKind::GtEq => (3, SyntaxKind::ComparisonExpr),
            TokenKind::Plus | TokenKind::Minus => (4, SyntaxKind::AdditiveExpr),
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent => {
                (5, SyntaxKind::MultiplicativeExpr)
            }
            _ => return None,
        })
    }

    /// Precedence climbing: operators above `min` bind here.
    fn expr(&mut self, min: u8) {
        let m = self.mark();
        self.unary();
        while let Some(k) = self.peek() {
            let Some((prec, node)) = Self::binop(k) else {
                break;
            };
            if prec <= min {
                break;
            }
            self.wrap(m, node);
            self.bump();
            self.expr(prec);
            self.finish();
        }
    }

    fn unary(&mut self) {
        if matches!(
            self.peek(),
            Some(TokenKind::Minus | TokenKind::Plus | TokenKind::Bang)
        ) {
            self.start(SyntaxKind::UnaryExpr);
            self.bump();
            self.unary();
            self.finish();
            return;
        }
        self.postfix();
    }

    fn postfix(&mut self) {
        let m = self.mark();
        self.primary();
        loop {
            if self.at(TokenKind::LParen) {
                self.wrap(m, SyntaxKind::CallExpr);
                self.args();
                self.finish();
            } else if self.at(TokenKind::Dot) && self.nth(1) == Some(TokenKind::Ident) {
                self.wrap(m, SyntaxKind::FieldExpr);
                self.bump();
                self.bump();
                self.finish();
            } else {
                break;
            }
        }
    }

    fn args(&mut self) {
        self.start(SyntaxKind::ArgumentList);
        self.bump();
        while !self.at(TokenKind::RParen) && self.peek().is_some() {
            self.start(SyntaxKind::Argument);
            self.expr(0);
            self.finish();
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        if !self.eat(TokenKind::RParen) {
            self.skip_to_separator();
            self.eat(TokenKind::RParen);
        }
        self.finish();
    }

    fn primary(&mut self) {
        match self.peek() {
            Some(
                TokenKind::Number | TokenKind::KwTrue | TokenKind::KwFalse | TokenKind::String,
            ) => {
                self.start(SyntaxKind::LiteralExpr);
                self.bump();
                self.finish();
            }
            Some(TokenKind::LParen) => {
                self.start(SyntaxKind::TupleExpr);
                self.bump();
                if !self.at(TokenKind::RParen) {
                    self.expr(0);
                }
                if !self.eat(TokenKind::RParen) {
                    self.skip_to_separator();
                    self.eat(TokenKind::RParen);
                }
                self.finish();
            }
            Some(TokenKind::Ident | TokenKind::KwSelfValue) => {
                let m = self.mark();
                self.start(SyntaxKind::NameExpr);
                self.bump();
                self.finish();
                if self.at(TokenKind::LBrace) {
                    self.wrap(m, SyntaxKind::DataExpr);
                    self.bump();
                    while !self.at(TokenKind::RBrace) && self.peek().is_some() {
                        self.start(SyntaxKind::NamedArgument);
                        if self.eat(TokenKind::Ident) && self.eat(TokenKind::Colon) {
                            self.expr(0);
                        } else {
                            self.skip_to_separator();
                            if self.peek().is_some()
                                && !self.at(TokenKind::RBrace)
                                && !self.at(TokenKind::Comma)
                            {
                                self.junk();
                            }
                        }
                        self.finish();
                        if !self.eat(TokenKind::Comma) {
                            break;
                        }
                    }
                    if !self.eat(TokenKind::RBrace) {
                        self.rest();
                    }
                    self.finish();
                }
            }
            Some(_) => self.junk(),
            None => {
                self.start(SyntaxKind::Error);
                self.finish();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{SyntaxKind, parse};

    fn kinds(src: &str) -> Vec<SyntaxKind> {
        let p = parse(src.as_bytes());
        p.tree
            .root()
            .descendants()
            .map(crate::NodeRef::kind)
            .collect()
    }

    #[test]
    fn statements_and_expressions_nest() {
        let k = kinds(
            "fn f[T < Shape](a: T, b: i32) -> i32:\n    x := a.area() + 2 * b\n    return x\n",
        );
        for want in [
            SyntaxKind::GenericParameter,
            SyntaxKind::Parameter,
            SyntaxKind::NamedType,
            SyntaxKind::LetStmt,
            SyntaxKind::AdditiveExpr,
            SyntaxKind::MultiplicativeExpr,
            SyntaxKind::CallExpr,
            SyntaxKind::FieldExpr,
            SyntaxKind::ReturnStmt,
        ] {
            assert!(k.contains(&want), "{want:?} in {k:?}");
        }
    }

    #[test]
    fn junk_never_unbalances() {
        for src in [
            "fn (:\n",
            "x := )\n",
            "impl for:\n",
            "p := P { x: , }\n",
            "if :\n    ]\n",
        ] {
            let p = parse(src.as_bytes());
            assert_eq!(p.tree.reconstruct(&p.tokens, src), src);
        }
    }
}
