//! Patterns (02-grammar.md: Patterns, Labels).

use hd_diag::Code;

use super::{Done, Parser};
use crate::{SyntaxKind, TokenKind};

impl Parser<'_> {
    /// A pattern; `mut` before a name is valid only where `let_pattern`.
    pub(crate) fn pattern(&mut self, let_pattern: bool) -> Done {
        if !self.enter() {
            let marker = self.start();
            self.skip_balanced();
            return self.complete(marker, SyntaxKind::Error);
        }
        let done = self.pattern_inner(let_pattern);
        self.leave();
        done
    }

    fn pattern_inner(&mut self, let_pattern: bool) -> Done {
        match self.current() {
            Some(TokenKind::Placeholder) => {
                let marker = self.start();
                self.bump();
                if self.at(TokenKind::Ellipsis) {
                    self.bump();
                    return self.complete(marker, SyntaxKind::SpreadPattern);
                }
                self.complete(marker, SyntaxKind::WildcardPattern)
            }
            Some(TokenKind::Minus | TokenKind::Number) => self.literal_or_range_pattern(),
            Some(TokenKind::DotDot | TokenKind::DotDotEq) => {
                let marker = self.start();
                self.bump();
                self.range_bound();
                self.complete(marker, SyntaxKind::RangePattern)
            }
            Some(TokenKind::KwTrue | TokenKind::KwFalse) => {
                self.token_node(SyntaxKind::LiteralPattern)
            }
            Some(TokenKind::Char) => {
                let done = self.token_node(SyntaxKind::LiteralPattern);
                if matches!(
                    self.current(),
                    Some(TokenKind::DotDot | TokenKind::DotDotEq)
                ) {
                    // grammar.pattern.range.bound
                    self.error(Code::SyntaxError);
                }
                done
            }
            Some(TokenKind::String) => {
                if self.prefixed_string(0) {
                    // grammar.pattern.no-literal-call
                    self.error(Code::SyntaxError);
                }
                self.token_node(SyntaxKind::LiteralPattern)
            }
            Some(TokenKind::Dot) => {
                let marker = self.start();
                self.bump();
                self.expect_name();
                if self.at(TokenKind::LParen) && !self.nth_line_first(0) {
                    self.pattern_arguments();
                }
                self.complete(marker, SyntaxKind::VariantPattern)
            }
            Some(TokenKind::KwMut) => {
                let marker = self.start();
                if !let_pattern {
                    // grammar.pattern.mut-let-only
                    self.error(Code::SyntaxError);
                }
                self.bump();
                self.expect_name();
                if self.at(TokenKind::Ellipsis) {
                    self.bump();
                    return self.complete(marker, SyntaxKind::SpreadPattern);
                }
                self.complete(marker, SyntaxKind::BindingPattern)
            }
            Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfType) => {
                self.name_pattern(let_pattern)
            }
            Some(TokenKind::LParen) => self.tuple_pattern(let_pattern),
            _ => {
                let marker = self.start();
                self.error(Code::SyntaxError);
                if self.current().is_some_and(|kind| {
                    !kind.closes()
                        && !matches!(
                            kind,
                            TokenKind::Comma
                                | TokenKind::Colon
                                | TokenKind::Eq
                                | TokenKind::FatArrow
                                | TokenKind::KwIn
                        )
                }) {
                    self.bump();
                }
                self.complete(marker, SyntaxKind::Error)
            }
        }
    }

    fn literal_or_range_pattern(&mut self) -> Done {
        let marker = self.start();
        let integer = self.range_bound();
        if matches!(
            self.current(),
            Some(TokenKind::DotDot | TokenKind::DotDotEq)
        ) {
            if !integer {
                self.error(Code::SyntaxError);
            }
            let inclusive = self.at(TokenKind::DotDotEq);
            self.bump();
            if matches!(self.current(), Some(TokenKind::Minus | TokenKind::Number)) {
                self.range_bound();
            } else if inclusive {
                self.error(Code::SyntaxError);
            }
            return self.complete(marker, SyntaxKind::RangePattern);
        }
        self.complete(marker, SyntaxKind::LiteralPattern)
    }

    /// `[-] number`; true for an integer.
    fn range_bound(&mut self) -> bool {
        self.eat(TokenKind::Minus);
        if !self.at(TokenKind::Number) {
            self.error(Code::SyntaxError);
            return false;
        }
        let text = self.nth_text(0);
        if number_has_suffix(text) {
            // grammar.pattern.no-literal-call
            self.error(Code::SyntaxError);
        }
        let integer = !text.contains('.') && !is_decimal_exponent(text);
        self.bump();
        integer
    }

    fn name_pattern(&mut self, let_pattern: bool) -> Done {
        let marker = self.start();
        self.bump();
        let mut qualified = false;
        while self.at(TokenKind::Dot)
            && matches!(self.nth(1), Some(TokenKind::Ident | TokenKind::RawIdent))
        {
            self.bump();
            self.bump();
            qualified = true;
        }
        if self.at(TokenKind::LBrace) && !self.nth_line_first(0) {
            self.data_pattern_fields(let_pattern);
            return self.complete(marker, SyntaxKind::DataPattern);
        }
        if self.at(TokenKind::LParen) && !self.nth_line_first(0) {
            self.pattern_arguments();
            return self.complete(marker, SyntaxKind::VariantPattern);
        }
        if qualified {
            return self.complete(marker, SyntaxKind::VariantPattern);
        }
        if self.at(TokenKind::Ellipsis) {
            self.bump();
            return self.complete(marker, SyntaxKind::SpreadPattern);
        }
        self.complete(marker, SyntaxKind::BindingPattern)
    }

    /// `( positional, ..., name = pattern, ... )`.
    fn pattern_arguments(&mut self) {
        let marker = self.start();
        self.bump(); // (
        let mut named = false;
        while !self.at(TokenKind::RParen) && self.current().is_some() {
            let before = self.pos();
            if matches!(self.current(), Some(TokenKind::Ident | TokenKind::RawIdent))
                && self.nth(1) == Some(TokenKind::Eq)
            {
                named = true;
                let field = self.start();
                self.bump();
                self.bump();
                self.pattern(false);
                self.complete(field, SyntaxKind::NamedPattern);
            } else {
                if named {
                    // grammar.pattern.named-last
                    let at = self.pos();
                    self.error_at(Code::PatternOrder, at);
                }
                self.pattern(false);
            }
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RParen) {
                    self.error(Code::SyntaxError);
                    self.skip_to_list_end(TokenKind::RParen);
                }
                break;
            }
            if self.pos() == before {
                break;
            }
        }
        self.expect(TokenKind::RParen);
        self.complete(marker, SyntaxKind::PatternArgumentList);
    }

    fn data_pattern_fields(&mut self, let_pattern: bool) {
        self.bump(); // {
        while !self.at(TokenKind::RBrace) && self.current().is_some() {
            let before = self.pos();
            let field = self.start();
            if self.at(TokenKind::KwMut) {
                if !let_pattern {
                    self.error(Code::SyntaxError);
                }
                self.bump();
                self.expect_name();
            } else {
                self.expect_name();
                if self.eat(TokenKind::Colon) {
                    self.pattern(let_pattern);
                } else if self.at(TokenKind::Eq) {
                    // grammar.label.data-pattern-equals
                    self.error(Code::SyntaxError);
                    self.bump();
                    self.pattern(let_pattern);
                }
            }
            self.complete(field, SyntaxKind::DataPatternField);
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RBrace) {
                    self.error(Code::SyntaxError);
                    self.skip_to_list_end(TokenKind::RBrace);
                }
                break;
            }
            if self.pos() == before {
                break;
            }
        }
        self.expect(TokenKind::RBrace);
    }

    /// `()`, `(p,)`, `(p, q, rest...)`; `(p)` alone is no pattern.
    fn tuple_pattern(&mut self, let_pattern: bool) -> Done {
        let marker = self.start();
        self.bump(); // (
        let mut count = 0;
        let mut comma = false;
        let mut spread_seen = false;
        while !self.at(TokenKind::RParen) && self.current().is_some() {
            let before = self.pos();
            if spread_seen {
                // grammar.pattern.tuple-spread: a spread is last.
                self.error(Code::SyntaxError);
            }
            let element = self.pattern(let_pattern);
            spread_seen |= element.kind() == SyntaxKind::SpreadPattern;
            count += 1;
            if !self.eat(TokenKind::Comma) {
                break;
            }
            comma = true;
            if self.pos() == before {
                break;
            }
        }
        if count > 0 && !comma {
            // grammar.stmt.let-pattern.parenthesized, grammar.pattern.tuple-spread
            self.error(Code::SyntaxError);
        }
        self.expect(TokenKind::RParen);
        self.complete(marker, SyntaxKind::TuplePattern)
    }
}

/// A decimal literal followed by a suffix name (`5px`, `1.5kb`).
pub(crate) fn number_has_suffix(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() > 1
        && bytes[0] == b'0'
        && matches!(bytes[1], b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
    {
        return false;
    }
    let mut index = 0;
    while index < bytes.len()
        && (bytes[index].is_ascii_digit() || matches!(bytes[index], b'_' | b'.'))
    {
        index += 1;
    }
    if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
        let mut look = index + 1;
        if look < bytes.len() && matches!(bytes[look], b'+' | b'-') {
            look += 1;
        }
        if look < bytes.len() && bytes[look].is_ascii_digit() {
            index = look;
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'_') {
                index += 1;
            }
        }
    }
    index < bytes.len()
}

fn is_decimal_exponent(text: &str) -> bool {
    let bytes = text.as_bytes();
    !(bytes.len() > 1 && bytes[0] == b'0' && matches!(bytes[1], b'x' | b'X'))
        && bytes.iter().any(|byte| matches!(byte, b'e' | b'E'))
}
