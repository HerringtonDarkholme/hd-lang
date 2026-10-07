//! Types and requirement rows (02-grammar.md: Types).

use hd_diag::Code;

use super::{Done, Parser};
use crate::{SyntaxKind, TokenKind};

impl Parser<'_> {
    /// `type`: function types own a requirement clause.
    pub(crate) fn ty(&mut self) -> Done {
        self.ty_with(true)
    }

    /// `result_type`: a function type here carries no requirement clause,
    /// so a header's clause belongs to the header.
    pub(crate) fn ty_result(&mut self) -> Done {
        self.ty_with(false)
    }

    fn ty_with(&mut self, rows: bool) -> Done {
        if !self.enter() {
            let marker = self.start();
            self.skip_balanced();
            return self.complete(marker, SyntaxKind::Error);
        }
        let done = self.ty_inner(rows);
        self.leave();
        done
    }

    fn ty_inner(&mut self, rows: bool) -> Done {
        if self.at(TokenKind::KwFn) {
            return self.ty_function(rows);
        }
        let marker = self.start();
        let mut done = if self.at(TokenKind::KwMut) {
            self.bump();
            if self.at(TokenKind::KwFn)
                || self.at(TokenKind::LParen) && self.nth(1) == Some(TokenKind::KwFn)
            {
                // grammar.type.mut.no-function
                self.error(Code::SyntaxError);
            }
            self.ty_reference(rows);
            self.complete(marker, SyntaxKind::MutType)
        } else {
            self.abandon(marker);
            self.ty_reference(rows)
        };
        while self.at(TokenKind::Question) {
            let marker = self.precede(done);
            self.bump();
            done = self.complete(marker, SyntaxKind::OptionalType);
        }
        done
    }

    fn ty_reference(&mut self, rows: bool) -> Done {
        match self.current() {
            Some(TokenKind::KwDyn) => {
                let marker = self.start();
                self.bump();
                if self.at(TokenKind::KwMut) {
                    // grammar.type.dyn.mut
                    self.error(Code::SyntaxError);
                    self.bump();
                }
                self.ty_bound_trait(true);
                self.complete(marker, SyntaxKind::DynType)
            }
            Some(TokenKind::LParen) => self.ty_paren(rows),
            Some(TokenKind::Dollar) if self.nth(1) == Some(TokenKind::Dot) => {
                let marker = self.start();
                self.bump();
                self.bump();
                if !(self.at(TokenKind::Ident) && self.nth_text(0) == "Context") {
                    self.error(Code::SyntaxError);
                }
                self.bump();
                if self.expect(TokenKind::LBracket) {
                    self.ty_argument();
                    self.expect(TokenKind::RBracket);
                }
                self.complete(marker, SyntaxKind::ContextType)
            }
            Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfType) => {
                let done = self.ty_named(true);
                if self.at(TokenKind::ColonColon) {
                    let marker = self.precede(done);
                    self.bump();
                    self.expect_name();
                    return self.complete(marker, SyntaxKind::ProjectionType);
                }
                done
            }
            _ => {
                let marker = self.start();
                self.error(Code::SyntaxError);
                if self.current().is_some_and(|kind| {
                    !kind.closes()
                        && !matches!(kind, TokenKind::Comma | TokenKind::Colon | TokenKind::Eq)
                }) {
                    self.bump();
                }
                self.complete(marker, SyntaxKind::Error)
            }
        }
    }

    /// `qualified_name [type arguments]` (or `Self`).
    pub(crate) fn ty_named(&mut self, bindings: bool) -> Done {
        let marker = self.start();
        if !self.eat(TokenKind::KwSelfType) {
            self.expect_name();
            while self.at(TokenKind::Dot)
                && matches!(self.nth(1), Some(TokenKind::Ident | TokenKind::RawIdent))
            {
                self.bump();
                self.bump();
            }
        }
        if self.at(TokenKind::LBracket) {
            self.ty_arguments(bindings);
        }
        self.complete(marker, SyntaxKind::NamedType)
    }

    /// `bound_trait_type`: a trait with arguments that may end in bindings.
    pub(crate) fn ty_bound_trait(&mut self, bindings: bool) -> Done {
        if self.at(TokenKind::KwMut) {
            // grammar.type.row.no-mut-key and friends.
            self.error(Code::SyntaxError);
            self.bump();
        }
        self.ty_named(bindings)
    }

    /// `[ type_argument, ..., Name = type, ... ]`.
    pub(crate) fn ty_arguments(&mut self, bindings: bool) -> Done {
        let marker = self.start();
        self.bump(); // [
        let mut saw_binding = false;
        while !self.at(TokenKind::RBracket) && self.current().is_some() {
            let before = self.pos();
            if matches!(self.current(), Some(TokenKind::Ident | TokenKind::RawIdent))
                && self.nth(1) == Some(TokenKind::Eq)
            {
                if !bindings {
                    // grammar.generic.binding.trait-type-only
                    self.error(Code::SyntaxError);
                }
                saw_binding = true;
                let binding = self.start();
                self.bump();
                self.bump();
                self.ty();
                self.complete(binding, SyntaxKind::AssociatedTypeBinding);
            } else {
                if saw_binding {
                    // grammar.generic.binding.order
                    self.error(Code::SyntaxError);
                }
                self.ty_argument();
                if self.at(TokenKind::Ellipsis) {
                    // grammar.type.rest.elsewhere
                    self.error(Code::SyntaxError);
                    self.bump();
                }
            }
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RBracket) {
                    self.error(Code::SyntaxError);
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
        self.complete(marker, SyntaxKind::TypeArgumentList)
    }

    /// `type | $ row`.
    pub(crate) fn ty_argument(&mut self) -> Done {
        if self.at(TokenKind::Dollar) && self.nth(1) != Some(TokenKind::Dot) {
            return self.requirement_clause();
        }
        if self.at(TokenKind::Placeholder) {
            // grammar.primary.placeholder: `_` is no type.
            self.error(Code::SyntaxError);
            return self.token_node(SyntaxKind::InferType);
        }
        self.ty()
    }

    /// `( )`, `( type )`, `( type, ... )`, with a rest element last.
    fn ty_paren(&mut self, rows: bool) -> Done {
        let _ = rows;
        let marker = self.start();
        self.bump(); // (
        if self.eat(TokenKind::RParen) {
            return self.complete(marker, SyntaxKind::TupleType);
        }
        let mut count = 0;
        let mut comma = false;
        let mut rest_seen = false;
        while !self.at(TokenKind::RParen) && self.current().is_some() {
            let before = self.pos();
            if rest_seen {
                // grammar.type.rest: a rest element is last.
                self.error(Code::SyntaxError);
            }
            let element = self.ty();
            if self.at(TokenKind::Ellipsis) {
                let rest = self.precede(element);
                self.bump();
                self.complete(rest, SyntaxKind::RestType);
                rest_seen = true;
            }
            count += 1;
            if !self.eat(TokenKind::Comma) {
                break;
            }
            comma = true;
            if self.pos() == before {
                break;
            }
        }
        if rest_seen && !comma {
            // `(T...)`: a rest element alone keeps the comma.
            self.error(Code::SyntaxError);
        }
        self.expect(TokenKind::RParen);
        let kind = if count == 1 && !comma {
            SyntaxKind::ParenType
        } else {
            SyntaxKind::TupleType
        };
        self.complete(marker, kind)
    }

    /// `fn [!] ( types ) -> type [$ row]`.
    fn ty_function(&mut self, rows: bool) -> Done {
        let marker = self.start();
        self.bump(); // fn
        if self.at(TokenKind::Bang) {
            self.bump();
        }
        if self.expect(TokenKind::LParen) {
            let mut rest_seen = false;
            while !self.at(TokenKind::RParen) && self.current().is_some() {
                let before = self.pos();
                if rest_seen {
                    self.error(Code::SyntaxError);
                }
                let element = self.ty();
                if self.at(TokenKind::Ellipsis) {
                    let rest = self.precede(element);
                    self.bump();
                    self.complete(rest, SyntaxKind::RestType);
                    rest_seen = true;
                }
                if !self.eat(TokenKind::Comma) || self.pos() == before {
                    break;
                }
            }
            self.expect(TokenKind::RParen);
        }
        if self.expect(TokenKind::Arrow) {
            self.ty_with(rows);
        }
        if rows && self.at(TokenKind::Dollar) {
            self.requirement_clause();
        }
        let done = self.complete(marker, SyntaxKind::FunctionType);
        if self.at(TokenKind::Question) {
            // grammar.type.optional.function: `?` needs a group.
            self.error(Code::SyntaxError);
        }
        done
    }

    /// `$ key + key`, `$()`; the old `$(A, B)` and `$ A, B` forms report.
    pub(crate) fn requirement_clause(&mut self) -> Done {
        let marker = self.start();
        self.bump(); // $
        if self.at(TokenKind::LParen) {
            self.bump();
            if !self.at(TokenKind::RParen) {
                self.ty_bound_trait(true);
                loop {
                    if self.at(TokenKind::Comma) {
                        // grammar.type.row.old-separator
                        let at = self.pos();
                        self.error_at(Code::OldRowSeparator, at);
                        self.bump();
                    } else if self.at(TokenKind::Plus) {
                        // grammar.type.row.no-parentheses
                        self.error(Code::SyntaxError);
                        self.bump();
                    } else {
                        break;
                    }
                    self.ty_bound_trait(true);
                }
            }
            self.expect(TokenKind::RParen);
        } else {
            self.ty_bound_trait(true);
            while self.eat(TokenKind::Plus) {
                self.ty_bound_trait(true);
            }
        }
        self.complete(marker, SyntaxKind::RequirementRow)
    }
}
