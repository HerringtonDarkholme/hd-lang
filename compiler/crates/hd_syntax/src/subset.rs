//! Walking-skeleton parser: recursive descent for items and statements, Pratt
//! for expressions, over the layout cursor, emitting the green tree's events.
//! It covers only the skeleton subset (future-work/compiler/skeleton-findings.md).

use hd_base::{NodeIdx, TokenIdx};
use hd_diag::Code;

use crate::green::{Event, build};
use crate::{GreenTree, Layout, LayoutCursor, SyntaxKind, TokenBuf, TokenKind, TokenOrLayout, lex};

pub struct SubsetParse {
    pub tokens: TokenBuf,
    pub tree: GreenTree,
    pub errors: Vec<String>,
}

#[must_use]
pub fn parse_subset(source: &str) -> SubsetParse {
    let lexed = lex(source.as_bytes());
    let mut parser = P {
        source,
        cursor: LayoutCursor::new(&lexed.tokens),
        tokens: &lexed.tokens,
        events: Vec::new(),
        layouts: Vec::new(),
        errors: Vec::new(),
        fuel: 1_000_000,
    };
    parser.file();
    let errors = parser.errors;
    let tree = build(&parser.events, &parser.layouts, &[] as &[(NodeIdx, Code)]);
    SubsetParse { tokens: lexed.tokens, tree, errors }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Peek {
    Tok(TokenKind),
    Lay(Layout),
    Eof,
}

struct P<'t> {
    source: &'t str,
    cursor: LayoutCursor<'t>,
    tokens: &'t TokenBuf,
    events: Vec<Event>,
    layouts: Vec<(TokenIdx, Layout)>,
    errors: Vec<String>,
    fuel: u32,
}

impl P<'_> {
    fn peek(&mut self) -> Peek {
        self.fuel = self.fuel.saturating_sub(1);
        assert!(self.fuel > 0, "parser made no progress");
        match self.cursor.peek() {
            TokenOrLayout::Token(t) => Peek::Tok(self.tokens.kind(t)),
            TokenOrLayout::Layout { kind, .. } => Peek::Lay(kind),
            TokenOrLayout::Eof => Peek::Eof,
        }
    }
    fn at(&mut self, kind: TokenKind) -> bool {
        self.peek() == Peek::Tok(kind)
    }
    fn bump(&mut self) {
        match self.cursor.peek() {
            TokenOrLayout::Token(t) => self.events.push(Event::Token(t)),
            TokenOrLayout::Layout { kind, at } => self.layouts.push((at, kind)),
            TokenOrLayout::Eof => {}
        }
        self.cursor.bump();
    }
    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, kind: TokenKind) {
        if !self.eat(kind) {
            let found = self.peek();
            self.errors.push(format!("expected {kind:?}, found {found:?}"));
            // Recover by skipping one token so the parser always progresses.
            if found != Peek::Eof {
                self.bump();
            }
        }
    }
    fn start(&mut self, kind: SyntaxKind) {
        self.events.push(Event::Start(kind));
    }
    fn finish(&mut self) {
        self.events.push(Event::Finish);
    }
    /// Index into `events` where a node may later be wrapped (Pratt's precede).
    fn mark(&self) -> usize {
        self.events.len()
    }
    fn wrap(&mut self, mark: usize, kind: SyntaxKind) {
        self.events.insert(mark, Event::Start(kind));
    }

    fn file(&mut self) {
        self.start(SyntaxKind::Root);
        loop {
            match self.peek() {
                Peek::Eof => break,
                Peek::Lay(_) => self.bump(),
                Peek::Tok(_) => self.item(),
            }
        }
        self.finish();
    }

    fn item(&mut self) {
        let mark = self.mark();
        let _ = self.eat(TokenKind::KwPub);
        match self.peek() {
            Peek::Tok(TokenKind::KwFn) => {
                self.wrap(mark, SyntaxKind::FnDecl);
                self.fn_rest(true);
                return; // fn_rest finishes the FnDecl
            }
            Peek::Tok(TokenKind::KwData) => {
                self.wrap(mark, SyntaxKind::DataDecl);
                self.bump();
                self.expect(TokenKind::Ident);
                self.expect(TokenKind::Colon);
                self.suite(Self::data_field);
            }
            Peek::Tok(TokenKind::KwTrait) => {
                self.wrap(mark, SyntaxKind::TraitDecl);
                self.bump();
                self.expect(TokenKind::Ident);
                self.expect(TokenKind::Colon);
                self.suite(|p| {
                    p.start(SyntaxKind::FnDecl);
                    p.fn_rest(false);
                });
            }
            Peek::Tok(TokenKind::KwImpl) => {
                self.wrap(mark, SyntaxKind::ImplDecl);
                self.bump();
                self.ty();
                if self.eat(TokenKind::KwFor) {
                    self.ty();
                }
                self.expect(TokenKind::Colon);
                self.suite(|p| {
                    p.start(SyntaxKind::FnDecl);
                    let _ = p.eat(TokenKind::KwPub);
                    p.fn_rest(true);
                });
            }
            Peek::Tok(TokenKind::Ident) if self.use_keyword() => {
                self.wrap(mark, SyntaxKind::UseDecl);
                self.bump();
                self.expect(TokenKind::Ident);
                while self.eat(TokenKind::Dot) {
                    if self.eat(TokenKind::LBrace) {
                        loop {
                            self.expect(TokenKind::Ident);
                            if !self.eat(TokenKind::Comma) {
                                break;
                            }
                        }
                        self.expect(TokenKind::RBrace);
                        break;
                    }
                    self.expect(TokenKind::Ident);
                }
            }
            other => {
                self.wrap(mark, SyntaxKind::Error);
                self.errors.push(format!("unexpected {other:?} at item start"));
                self.bump();
            }
        }
        self.finish();
    }

    fn use_keyword(&mut self) -> bool {
        // `use` is a contextual identifier in this lexer.
        match self.cursor.peek() {
            TokenOrLayout::Token(t) => self.tokens.text(t, self.source) == "use",
            _ => false,
        }
    }

    /// `fn name[generics](params) -> T` and, when `body`, `: suite`. The
    /// `FnDecl` node is already started by the caller.
    fn fn_rest(&mut self, body: bool) {
        self.expect(TokenKind::KwFn);
        self.expect(TokenKind::Ident);
        if self.at(TokenKind::LBracket) {
            self.start(SyntaxKind::GenericParameterList);
            self.bump();
            loop {
                self.start(SyntaxKind::GenericParameter);
                self.expect(TokenKind::Ident);
                if self.eat(TokenKind::Lt) {
                    self.ty();
                }
                self.finish();
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::RBracket);
            self.finish();
        }
        self.start(SyntaxKind::ParameterList);
        self.expect(TokenKind::LParen);
        while !self.at(TokenKind::RParen) && self.peek() != Peek::Eof {
            self.start(SyntaxKind::Parameter);
            if !self.eat(TokenKind::KwSelfValue) {
                self.expect(TokenKind::Ident);
                self.expect(TokenKind::Colon);
                self.ty();
            }
            self.finish();
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen);
        self.finish();
        if self.eat(TokenKind::Arrow) {
            self.ty();
        }
        if body {
            self.expect(TokenKind::Colon);
            self.block();
        }
        self.finish();
    }

    fn data_field(&mut self) {
        self.start(SyntaxKind::DataField);
        let _ = self.eat(TokenKind::KwPub);
        self.expect(TokenKind::Ident);
        self.expect(TokenKind::Colon);
        self.ty();
        self.finish();
    }

    fn ty(&mut self) {
        self.start(SyntaxKind::NamedType);
        if !self.eat(TokenKind::KwSelfType) {
            self.expect(TokenKind::Ident);
        }
        self.finish();
    }

    /// An indented suite after a colon: `Newline Indent (elem Newline)* Dedent`.
    fn suite(&mut self, mut elem: impl FnMut(&mut Self)) {
        self.cursor.open_suite(false);
        if self.peek() != Peek::Lay(Layout::Newline) {
            elem(self);
            return;
        }
        self.bump();
        if self.peek() != Peek::Lay(Layout::Indent) {
            self.errors.push("expected an indented block".into());
            return;
        }
        self.bump();
        loop {
            match self.peek() {
                Peek::Lay(Layout::Newline | Layout::SuiteEnd) => self.bump(),
                Peek::Lay(Layout::Dedent) => {
                    self.bump();
                    break;
                }
                Peek::Eof => break,
                Peek::Lay(Layout::Indent) => {
                    self.errors.push("unexpected indent".into());
                    self.bump();
                }
                Peek::Tok(_) => elem(self),
            }
        }
    }

    fn block(&mut self) {
        self.start(SyntaxKind::Block);
        self.suite(Self::stmt);
        self.finish();
    }

    fn stmt(&mut self) {
        match self.peek() {
            Peek::Tok(TokenKind::KwReturn) => {
                self.start(SyntaxKind::ReturnStmt);
                self.bump();
                if matches!(self.peek(), Peek::Tok(_)) {
                    self.expr(0);
                }
                self.finish();
            }
            Peek::Tok(TokenKind::KwIf) => self.if_stmt(),
            Peek::Tok(TokenKind::KwWhile) => {
                self.start(SyntaxKind::WhileExpr);
                self.bump();
                self.expr(0);
                self.expect(TokenKind::Colon);
                self.block();
                self.finish();
            }
            _ => {
                let mark = self.mark();
                self.expr(0);
                if self.eat(TokenKind::ColonEq) {
                    self.wrap(mark, SyntaxKind::LetStmt);
                    self.expr(0);
                    self.finish();
                } else if self.eat(TokenKind::Eq) {
                    self.wrap(mark, SyntaxKind::AssignmentStmt);
                    self.expr(0);
                    self.finish();
                } else {
                    self.wrap(mark, SyntaxKind::ExprStmt);
                    self.finish();
                }
            }
        }
    }

    fn if_stmt(&mut self) {
        self.start(SyntaxKind::IfExpr);
        self.bump();
        self.expr(0);
        self.expect(TokenKind::Colon);
        self.block();
        if self.eat(TokenKind::KwElse) {
            if self.at(TokenKind::KwIf) {
                // `else if` nests as an if inside an else block.
                self.start(SyntaxKind::Block);
                self.if_stmt();
                self.finish();
            } else {
                self.expect(TokenKind::Colon);
                self.block();
            }
        }
        self.finish();
    }

    fn binop(kind: TokenKind) -> Option<(u8, SyntaxKind)> {
        Some(match kind {
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

    fn expr(&mut self, min: u8) {
        let mark = self.mark();
        self.unary();
        while let Peek::Tok(kind) = self.peek() {
            let Some((prec, node)) = Self::binop(kind) else { break };
            if prec <= min {
                break;
            }
            self.wrap(mark, node);
            self.bump();
            // Comparisons do not chain: parse the right side above them.
            self.expr(prec);
            self.finish();
        }
    }

    fn unary(&mut self) {
        if self.at(TokenKind::Minus) || self.at(TokenKind::Plus) {
            self.start(SyntaxKind::UnaryExpr);
            self.bump();
            self.unary();
            self.finish();
            return;
        }
        self.postfix();
    }

    fn postfix(&mut self) {
        let mark = self.mark();
        self.primary();
        loop {
            if self.at(TokenKind::LParen) {
                self.wrap(mark, SyntaxKind::CallExpr);
                self.args();
                self.finish();
            } else if self.at(TokenKind::Dot) {
                self.wrap(mark, SyntaxKind::FieldExpr);
                self.bump();
                self.expect(TokenKind::Ident);
                self.finish();
            } else {
                break;
            }
        }
    }

    fn args(&mut self) {
        self.start(SyntaxKind::ArgumentList);
        self.expect(TokenKind::LParen);
        while !self.at(TokenKind::RParen) && self.peek() != Peek::Eof {
            self.start(SyntaxKind::Argument);
            self.expr(0);
            self.finish();
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen);
        self.finish();
    }

    fn primary(&mut self) {
        match self.peek() {
            Peek::Tok(TokenKind::Number | TokenKind::KwTrue | TokenKind::KwFalse) => {
                self.start(SyntaxKind::LiteralExpr);
                self.bump();
                self.finish();
            }
            Peek::Tok(TokenKind::LParen) => {
                self.start(SyntaxKind::TupleExpr);
                self.bump();
                self.expr(0);
                self.expect(TokenKind::RParen);
                self.finish();
            }
            Peek::Tok(TokenKind::Ident | TokenKind::KwSelfValue) => {
                let mark = self.mark();
                self.start(SyntaxKind::NameExpr);
                self.bump();
                self.finish();
                if self.at(TokenKind::LBrace) {
                    self.wrap(mark, SyntaxKind::DataExpr);
                    self.bump();
                    while !self.at(TokenKind::RBrace) && self.peek() != Peek::Eof {
                        self.start(SyntaxKind::NamedArgument);
                        self.expect(TokenKind::Ident);
                        self.expect(TokenKind::Colon);
                        self.expr(0);
                        self.finish();
                        if !self.eat(TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::RBrace);
                    self.finish();
                }
            }
            other => {
                self.start(SyntaxKind::Error);
                self.errors.push(format!("expected an expression, found {other:?}"));
                if other != Peek::Eof {
                    self.bump();
                }
                self.finish();
            }
        }
    }
}
