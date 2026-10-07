//! Items: source files, use declarations, declarations and their members
//! (02-grammar.md: Source Files, Test Blocks, Declarations, Generic
//! Parameters, Use Declarations, Annotations).

use hd_diag::Code;

use super::{Ctx, Marker, Parser};
use crate::layout::SuiteKind;
use crate::{Layout, SyntaxKind, TokenKind};

/// Where an item stands.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    Top,
    Tests,
    Body,
}

/// What a function declaration belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    Free,
    Trait,
    InherentImpl,
    TraitImpl,
}

/// Which generic parameter list a declaration takes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Generics {
    /// `type_params`: variance and defaults.
    Type,
    /// `function_generic_params`: defaults, no variance.
    Function,
    /// `generic_params` of an implementation: neither.
    Impl,
}

/// Which parameter list.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Params {
    Function,
    Closure,
    Enum,
    Variant,
}

impl Parser<'_> {
    pub(crate) fn source_file(&mut self) {
        let root = self.start();
        loop {
            match self.layout() {
                Some(Layout::Newline | Layout::Dedent | Layout::SuiteEnd) => {
                    self.bump();
                    continue;
                }
                Some(Layout::Indent) => {
                    self.stray_indent();
                    continue;
                }
                None => {}
            }
            if self.at_eof() {
                break;
            }
            let before = self.pos();
            self.stmt_errored = false;
            self.item(Place::Top);
            self.statement_end();
            if self.pos() == before && self.layout().is_none() && !self.at_eof() {
                self.junk();
            }
        }
        self.complete(root, SyntaxKind::Root);
    }

    /// A top-level or `tests:` item: a use declaration, a declaration, or a
    /// statement.
    pub(crate) fn item(&mut self, place: Place) {
        if self.at_word("use") && self.use_root_follows() {
            self.use_decl();
            return;
        }
        if self.at(TokenKind::KwPub)
            && self.nth(1) == Some(TokenKind::Ident)
            && self.nth_text(1) == "use"
        {
            self.use_decl();
            return;
        }
        if self.at(TokenKind::Ident)
            && matches!(self.nth(1), Some(TokenKind::Ident))
            && !self.nth_line_first(1)
        {
            let code = match self.nth_text(0) {
                "import" => Some(Code::OldImportDeclaration),
                "export" => Some(Code::OldExportDeclaration),
                "struct" => Some(Code::OldStructDeclaration),
                _ => None,
            };
            if let Some(code) = code {
                self.error(code);
                self.recover_line();
                if self.eat_layout(Layout::Newline) && self.at_layout(Layout::Indent) {
                    self.stray_skip();
                }
                return;
            }
        }
        if self.at(TokenKind::KwTests) {
            if place == Place::Top {
                self.tests_block();
            } else {
                self.error(Code::SyntaxError);
                self.skip_item();
            }
            return;
        }
        if self.declaration_start() {
            self.declaration(place);
            return;
        }
        self.statement();
    }

    fn use_root_follows(&mut self) -> bool {
        if self.nth_line_first(1) {
            return false;
        }
        match self.nth(1) {
            Some(TokenKind::KwSelfValue) => true,
            Some(TokenKind::Ident) => {
                matches!(self.nth_text(1), "pkg" | "std" | "dep" | "super")
            }
            _ => false,
        }
    }

    /// An item and its indented body, as junk.
    fn skip_item(&mut self) {
        self.recover_line();
        if self.eat_layout(Layout::Newline) && self.at_layout(Layout::Indent) {
            self.stray_skip();
        }
    }

    /// Skips an indented region without reporting.
    fn stray_skip(&mut self) {
        let marker = self.start();
        let mut depth = 0_u32;
        loop {
            match self.layout() {
                Some(Layout::Indent) => depth += 1,
                Some(Layout::Dedent) => {
                    depth -= 1;
                    if depth == 0 {
                        self.bump();
                        break;
                    }
                }
                _ => {}
            }
            if self.at_eof() {
                break;
            }
            self.bump();
        }
        self.complete(marker, SyntaxKind::Error);
        self.suite_closed = true;
    }

    /// The current token starts a declaration (after decorators and `pub`).
    pub(crate) fn declaration_start(&mut self) -> bool {
        let mut n = 0;
        if self.at(TokenKind::At) {
            return true;
        }
        if self.at(TokenKind::KwPub) {
            n = 1;
        }
        match self.nth(n) {
            Some(TokenKind::KwFn) => matches!(
                self.nth(n + 1),
                Some(TokenKind::Ident | TokenKind::RawIdent)
            ),
            Some(
                TokenKind::KwData
                | TokenKind::KwEnum
                | TokenKind::KwTrait
                | TokenKind::KwImpl
                | TokenKind::KwType,
            ) => true,
            _ => n == 1,
        }
    }

    /// `[decorators] [pub] declaration`.
    pub(crate) fn declaration(&mut self, place: Place) {
        self.attach_docs();
        let marker = self.start();
        let decorated = self.decorators(place == Place::Body);
        let public = self.eat(TokenKind::KwPub);
        match self.current() {
            Some(TokenKind::KwFn) => self.fn_decl(marker, Owner::Free),
            Some(TokenKind::KwData) => self.data_decl(marker),
            Some(TokenKind::KwEnum) => self.enum_decl(marker),
            Some(TokenKind::KwTrait) => self.trait_decl(marker),
            Some(TokenKind::KwImpl) => {
                if public {
                    // grammar.decl.impl-no-pub
                    let at = self.pos() - 1;
                    self.error_at(Code::SyntaxError, at);
                }
                self.impl_decl(marker);
            }
            Some(TokenKind::KwType) => self.type_decl(marker, decorated),
            _ => {
                self.error(Code::SyntaxError);
                self.recover_line();
                self.complete(marker, SyntaxKind::Error);
            }
        }
    }

    /// Decorator lines, each `@ closed_expression NEWLINE`. True if any.
    pub(crate) fn decorators(&mut self, local: bool) -> bool {
        let mut any = false;
        while self.at(TokenKind::At) {
            any = true;
            if local {
                let at = self.pos();
                self.error_at(Code::DecoratorNotTopLevel, at);
            }
            let marker = self.start();
            self.bump();
            self.expr(Ctx::Closed);
            self.complete(marker, SyntaxKind::Decorator);
            if !self.eat_layout(Layout::Newline) {
                self.error(Code::SyntaxError);
                self.recover_line();
                self.eat_layout(Layout::Newline);
            }
            self.attach_docs();
        }
        any
    }

    // ---------------------------------------------------------------- use

    fn use_decl(&mut self) {
        self.attach_docs();
        let marker = self.start();
        let public = self.eat(TokenKind::KwPub);
        self.bump(); // use
        let mut grouped = false;
        // use_root
        if self.at_word("dep") {
            self.bump();
            if self.expect(TokenKind::Dot) {
                self.expect_name();
            }
        } else if self.at_word("super") {
            self.bump();
            while self.at(TokenKind::Dot) && self.nth_text(1) == "super" {
                self.bump();
                self.bump();
            }
        } else {
            self.bump();
        }
        while self.eat(TokenKind::Dot) {
            if self.at(TokenKind::LBrace) {
                self.use_group();
                grouped = true;
                break;
            }
            if !self.expect_name() {
                break;
            }
        }
        if !grouped && self.at_word("as") {
            self.bump();
            self.expect_name();
        }
        if public && !grouped {
            // grammar: `pub use` takes a group.
            let at = self.pos().saturating_sub(1);
            self.error_at(Code::SyntaxError, at);
        }
        self.complete(marker, SyntaxKind::UseDecl);
    }

    fn use_group(&mut self) {
        let marker = self.start();
        self.bump();
        while !self.at(TokenKind::RBrace) && self.current().is_some() {
            let item = self.start();
            if !self.expect_name() {
                self.abandon(item);
                break;
            }
            if self.at(TokenKind::Dot) {
                let at = self.pos();
                self.error_at(Code::DirectVariantUse, at);
                while self.eat(TokenKind::Dot) {
                    self.eat_name();
                }
            }
            if self.at_word("as") {
                self.bump();
                self.expect_name();
            }
            self.complete(item, SyntaxKind::UseItem);
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RBrace);
        self.complete(marker, SyntaxKind::UseGroup);
    }

    // ---------------------------------------------------------- functions

    /// `fn name[!] [generics] (params) [-> result] [$ row] (: suite | NEWLINE)`.
    pub(crate) fn fn_decl(&mut self, marker: Marker, owner: Owner) {
        self.bump(); // fn
        self.expect_name();
        if self.at(TokenKind::Bang) && self.nth_glued(0) {
            self.bump();
        }
        if self.at(TokenKind::LBracket) {
            self.generic_params(Generics::Function);
        }
        if self.at(TokenKind::LParen) {
            self.param_list(Params::Function);
        } else {
            self.error(Code::SyntaxError);
        }
        if self.eat(TokenKind::Arrow) {
            self.ty_result();
        }
        if self.at(TokenKind::Dollar) {
            self.requirement_clause();
            if self.at(TokenKind::Comma) {
                // grammar.type.row.old-separator
                let at = self.pos();
                self.error_at(Code::OldRowSeparator, at);
                while self.eat(TokenKind::Comma) {
                    self.ty_bound_trait(true);
                }
            }
        }
        if self.eat(TokenKind::Colon) {
            self.suite(false, Ctx::Inline);
        } else if !(self.at_layout(Layout::Newline)
            || self.at_eof()
            || self.at_layout(Layout::Dedent))
        {
            self.error(Code::SyntaxError);
            self.recover_line();
        } else if owner == Owner::Free {
            // A free function needs a body.
            self.error(Code::SyntaxError);
        }
        self.complete(marker, SyntaxKind::FnDecl);
    }

    /// `[ param, ... ]` of a declaration.
    pub(crate) fn generic_params(&mut self, mode: Generics) {
        let marker = self.start();
        self.bump(); // [
        while !self.at(TokenKind::RBracket) && self.current().is_some() {
            let before = self.pos();
            let param = self.start();
            if matches!(self.current(), Some(TokenKind::Plus | TokenKind::Minus))
                && mode == Generics::Type
            {
                self.bump();
            }
            let row = self.eat(TokenKind::Dollar);
            self.expect_name();
            if self.at(TokenKind::Lt) {
                if row {
                    // grammar.generic.row-marker.no-bound
                    self.error(Code::SyntaxError);
                }
                self.bound_list();
            } else if self.at(TokenKind::Colon) {
                // grammar.decl.bound-vs-colon
                self.error(Code::SyntaxError);
                self.bump();
                self.ty();
            }
            if self.at(TokenKind::Eq) {
                if mode == Generics::Impl {
                    // grammar.generic.default.impl
                    self.error(Code::SyntaxError);
                }
                let default = self.start();
                self.bump();
                self.ty_argument();
                self.complete(default, SyntaxKind::TypeDefault);
            }
            self.complete(param, SyntaxKind::GenericParameter);
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RBracket) {
                    self.error(Code::SyntaxError);
                    self.skip_to_list_end(TokenKind::RBracket);
                }
                break;
            }
            if self.pos() == before {
                break;
            }
        }
        self.expect(TokenKind::RBracket);
        self.complete(marker, SyntaxKind::GenericParameterList);
    }

    /// `< [mut] Bound & Bound ...` as a `BoundList`.
    pub(crate) fn bound_list(&mut self) {
        let marker = self.start();
        self.bump(); // <
        self.eat(TokenKind::KwMut);
        self.ty_bound_trait(true);
        loop {
            if self.eat(TokenKind::Amp) {
                self.ty_bound_trait(true);
            } else if self.at(TokenKind::Plus) {
                // grammar.generic.bound.old-plus
                let at = self.pos();
                self.error_at(Code::OldBoundOperator, at);
                self.bump();
                self.ty_bound_trait(true);
            } else {
                break;
            }
        }
        self.complete(marker, SyntaxKind::BoundList);
    }

    /// Skips to a closing token of the current list, at its depth.
    pub(crate) fn skip_to_list_end(&mut self, close: TokenKind) {
        let marker = self.start();
        let mut depth = 0_u32;
        while let Some(kind) = self.current() {
            if depth == 0 && (kind == close || kind == TokenKind::Comma || kind.closes()) {
                break;
            }
            if kind.opens() {
                depth += 1;
            } else if kind.closes() {
                depth -= 1;
            }
            self.bump();
        }
        self.complete(marker, SyntaxKind::Error);
    }

    /// A parenthesized parameter list.
    pub(crate) fn param_list(&mut self, mode: Params) {
        let marker = self.start();
        self.bump(); // (
        let mut named_default = false;
        while !self.at(TokenKind::RParen) && self.current().is_some() {
            let before = self.pos();
            self.parameter(mode, &mut named_default);
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RParen) {
                    self.error(Code::SyntaxError);
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
        self.complete(marker, SyntaxKind::ParameterList);
    }

    fn parameter(&mut self, mode: Params, _named_default: &mut bool) {
        self.attach_docs();
        let marker = self.start();
        while self.at(TokenKind::At) {
            let decorator = self.start();
            self.bump();
            self.expr(Ctx::Continued);
            self.complete(decorator, SyntaxKind::Decorator);
            self.attach_docs();
        }
        match mode {
            Params::Function => {
                if self.at(TokenKind::KwSelfValue) {
                    self.bump();
                } else if self.at(TokenKind::KwMut) {
                    self.bump();
                    if !self.eat(TokenKind::KwSelfValue) {
                        // grammar: only the receiver takes `mut`.
                        let at = self.pos() - 1;
                        self.error_at(Code::SyntaxError, at);
                        self.eat_name();
                        if self.eat(TokenKind::Colon) {
                            self.ty();
                        }
                    }
                } else {
                    self.expect_name();
                    let vararg = self.eat(TokenKind::Ellipsis);
                    if self.expect(TokenKind::Colon) {
                        self.ty();
                    }
                    if self.at(TokenKind::Ellipsis) {
                        // grammar.type.rest.elsewhere
                        self.error(Code::SyntaxError);
                        self.bump();
                    }
                    if self.at(TokenKind::Eq) {
                        if vararg {
                            // grammar.fn.vararg-name
                            self.error(Code::SyntaxError);
                        }
                        self.default_value(Ctx::Expr);
                    }
                }
            }
            Params::Closure => {
                self.expect_name();
                if self.eat(TokenKind::Colon) {
                    self.ty();
                }
            }
            Params::Enum | Params::Variant => {
                if matches!(self.current(), Some(TokenKind::Ident | TokenKind::RawIdent))
                    && self.nth(1) == Some(TokenKind::Colon)
                {
                    self.bump();
                    self.bump();
                }
                self.ty();
                if self.at(TokenKind::Eq) {
                    if mode == Params::Variant {
                        // grammar.enum.defaults
                        self.error(Code::SyntaxError);
                    }
                    self.default_value(Ctx::Expr);
                }
            }
        }
        self.complete(marker, SyntaxKind::Parameter);
    }

    /// `= expression` of a parameter or field.
    pub(crate) fn default_value(&mut self, ctx: Ctx) {
        let marker = self.start();
        self.bump(); // =
        self.expr(ctx);
        self.complete(marker, SyntaxKind::DefaultValue);
    }

    // --------------------------------------------------------- data, enum

    fn data_decl(&mut self, marker: Marker) {
        self.bump(); // data
        self.expect_name();
        if self.at(TokenKind::LBracket) {
            self.generic_params(Generics::Type);
        }
        if self.expect(TokenKind::Colon) {
            self.member_block(true, Self::data_member);
        }
        self.complete(marker, SyntaxKind::DataDecl);
    }

    fn data_member(&mut self) {
        self.attach_docs();
        if self.at(TokenKind::At) {
            let marker = self.start();
            self.decorators(false);
            self.data_field_or_embedded(marker);
            return;
        }
        let marker = self.start();
        self.data_field_or_embedded(marker);
    }

    fn data_field_or_embedded(&mut self, marker: Marker) {
        if self.at(TokenKind::KwMut) {
            // grammar.data.no-mut-modifier
            let at = self.pos();
            self.error_at(Code::MutableFieldModifier, at);
            self.bump();
        }
        let public = self.eat(TokenKind::KwPub);
        if matches!(self.current(), Some(TokenKind::Ident | TokenKind::RawIdent))
            && self.nth(1) == Some(TokenKind::Colon)
        {
            self.bump();
            self.bump();
            self.ty();
            if self.at(TokenKind::Eq) {
                self.default_value(Ctx::Closed);
            }
            self.complete(marker, SyntaxKind::DataField);
            return;
        }
        if public {
            // grammar.data.embedded.no-pub
            let at = self.pos() - 1;
            self.error_at(Code::SyntaxError, at);
        }
        if matches!(self.current(), Some(TokenKind::Ident)) {
            self.ty();
            self.complete(marker, SyntaxKind::EmbeddedField);
            return;
        }
        self.error(Code::SyntaxError);
        self.recover_line();
        self.complete(marker, SyntaxKind::Error);
    }

    fn enum_decl(&mut self, marker: Marker) {
        self.bump(); // enum
        self.expect_name();
        if self.at(TokenKind::LBracket) {
            self.generic_params(Generics::Type);
        }
        if self.at(TokenKind::LParen) {
            self.param_list(Params::Enum);
        }
        if self.expect(TokenKind::Colon) {
            self.member_block(false, Self::enum_variant);
        }
        self.complete(marker, SyntaxKind::EnumDecl);
    }

    fn enum_variant(&mut self) {
        self.attach_docs();
        let marker = self.start();
        self.decorators(false);
        self.attach_docs();
        self.expect_name();
        if self.at(TokenKind::LBracket) {
            // grammar.enum.no-type-params
            let at = self.pos();
            self.error_at(Code::VariantResultTypeRemoved, at);
            self.generic_params(Generics::Function);
        }
        if self.at(TokenKind::LParen) {
            self.param_list(Params::Variant);
        }
        if self.eat(TokenKind::Arrow) {
            let shared = self.start();
            let at = self.pos();
            self.expect_name();
            if self.at(TokenKind::LBracket) {
                // grammar.enum.no-result-type
                self.error_at(Code::VariantResultTypeRemoved, at);
                self.ty_arguments(true);
            }
            if self.at(TokenKind::LParen) {
                self.arguments();
            } else {
                self.error_at(Code::VariantResultTypeRemoved, at);
            }
            self.complete(shared, SyntaxKind::VariantSharedData);
        }
        if self.at(TokenKind::Colon) {
            // A variant takes no block.
            self.error(Code::SyntaxError);
            self.skip_item();
        }
        self.complete(marker, SyntaxKind::EnumVariant);
    }

    /// The indented member block after a declaration's `:`, as a `Block`.
    /// `pass` alone may stand on the header's line where `pass_inline`.
    pub(crate) fn member_block(&mut self, pass_inline: bool, mut member: impl FnMut(&mut Self)) {
        let block = self.start();
        match self.cur.open_suite(false) {
            SuiteKind::SameLine => {
                if !(pass_inline && self.eat(TokenKind::KwPass)) {
                    self.error(Code::SyntaxError);
                    self.recover_line();
                }
                self.cur.close_inline();
                self.suite_inline = true;
            }
            SuiteKind::Indented => {
                self.eat_layout(Layout::Newline);
                self.eat_layout(Layout::Indent);
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
                    if self.at(TokenKind::KwPass) {
                        self.token_node(SyntaxKind::LiteralExpr);
                    } else {
                        member(self);
                    }
                    self.statement_end();
                    if self.pos() == before && self.layout().is_none() && !self.at_eof() {
                        self.junk();
                    }
                }
                self.eat_layout(Layout::Dedent);
                self.suite_inline = false;
            }
            SuiteKind::Missing => self.error(Code::SyntaxError),
        }
        self.suite_closed = true;
        self.complete(block, SyntaxKind::Block);
    }

    // -------------------------------------------------------- trait, impl

    fn trait_decl(&mut self, marker: Marker) {
        self.bump(); // trait
        self.expect_name();
        if self.at(TokenKind::LBracket) {
            self.generic_params(Generics::Type);
        }
        if self.at(TokenKind::Lt) {
            self.bound_list();
        }
        if self.eat(TokenKind::Colon) {
            if self.nth_line_first(0) || self.at_layout(Layout::Newline) || self.at_eof() {
                self.member_block(false, Self::trait_member);
            } else {
                // grammar.trait.supertrait: `:` opens the body.
                self.error(Code::SyntaxError);
                self.recover_line();
            }
        }
        self.complete(marker, SyntaxKind::TraitDecl);
    }

    fn trait_member(&mut self) {
        self.attach_docs();
        if self.at(TokenKind::KwType) {
            self.associated_type(false);
            return;
        }
        let marker = self.start();
        self.decorators(false);
        if self.at(TokenKind::KwPub) {
            // grammar.impl.pub-method
            let at = self.pos();
            self.error_at(Code::TraitMethodVisibility, at);
            self.bump();
        }
        if self.at(TokenKind::KwFn) {
            self.fn_decl(marker, Owner::Trait);
        } else {
            self.error(Code::SyntaxError);
            self.recover_line();
            self.complete(marker, SyntaxKind::Error);
        }
    }

    fn associated_type(&mut self, with_value: bool) {
        let marker = self.start();
        self.bump(); // type
        self.expect_name();
        if self.at(TokenKind::Eq) {
            if !with_value {
                self.error(Code::SyntaxError);
            }
            self.bump();
            self.ty();
        }
        self.complete(marker, SyntaxKind::AssociatedTypeDecl);
    }

    fn impl_decl(&mut self, marker: Marker) {
        self.bump(); // impl
        if self.at(TokenKind::LBracket) {
            self.generic_params(Generics::Impl);
        }
        if matches!(
            self.current(),
            Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfType)
        ) {
            // A trait_type takes no binding (grammar.generic.binding.trait-type-only).
            let mut done = self.ty_named(false);
            while self.at(TokenKind::Question) {
                let marker = self.precede(done);
                self.bump();
                done = self.complete(marker, SyntaxKind::OptionalType);
            }
        } else {
            self.ty();
        }
        let trait_impl = self.eat(TokenKind::KwFor);
        if trait_impl {
            self.ty();
        }
        if self.at_word("by") {
            self.bump();
            self.expect_name();
        }
        let owner = if trait_impl {
            Owner::TraitImpl
        } else {
            Owner::InherentImpl
        };
        if self.eat(TokenKind::Colon) {
            self.member_block(false, |p| p.impl_member(owner));
        }
        self.complete(marker, SyntaxKind::ImplDecl);
    }

    fn impl_member(&mut self, owner: Owner) {
        self.attach_docs();
        if self.at(TokenKind::KwType) {
            self.associated_type(true);
            return;
        }
        let marker = self.start();
        self.decorators(false);
        if self.at(TokenKind::KwPub) {
            if owner == Owner::TraitImpl {
                let at = self.pos();
                self.error_at(Code::TraitMethodVisibility, at);
            }
            self.bump();
        }
        if self.at(TokenKind::KwFn) {
            self.fn_decl(marker, owner);
            return;
        }
        // derivation_line: (identifier | Self) (= | +=) closed_expression
        if matches!(
            self.current(),
            Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfType)
        ) && matches!(self.nth(1), Some(TokenKind::Eq | TokenKind::PlusEq))
        {
            self.bump();
            self.bump();
            self.expr(Ctx::Rhs);
            self.complete(marker, SyntaxKind::Derivation);
            return;
        }
        self.error(Code::SyntaxError);
        self.recover_line();
        self.complete(marker, SyntaxKind::Error);
    }

    // --------------------------------------------------------- type, tests

    fn type_decl(&mut self, marker: Marker, decorated: bool) {
        self.bump(); // type
        self.expect_name();
        if self.at(TokenKind::LBracket) {
            self.generic_params(Generics::Type);
        }
        if self.at(TokenKind::LParen) {
            self.bump();
            self.ty();
            self.expect(TokenKind::RParen);
        } else if self.eat(TokenKind::Eq) {
            if decorated {
                // grammar.annot.alias-no-decorator
                let at = self.pos() - 1;
                self.error_at(Code::SyntaxError, at);
            }
            if self.at(TokenKind::Dollar) {
                self.requirement_clause();
            } else {
                let row = self.start();
                let done = self.ty();
                if self.at(TokenKind::Plus) {
                    while self.eat(TokenKind::Plus) {
                        self.ty_bound_trait(true);
                    }
                    self.complete(row, SyntaxKind::RequirementRow);
                } else {
                    self.abandon(row);
                    let _ = done;
                }
            }
        } else {
            self.error(Code::SyntaxError);
        }
        self.complete(marker, SyntaxKind::TypeDecl);
    }

    fn tests_block(&mut self) {
        let marker = self.start();
        self.bump(); // tests
        if self.expect(TokenKind::Colon) {
            self.member_block(false, |p| {
                if p.at(TokenKind::KwTests) {
                    // grammar.tests.top-level
                    p.error(Code::SyntaxError);
                    p.skip_item();
                } else {
                    p.item(Place::Tests);
                }
            });
        }
        self.complete(marker, SyntaxKind::TestsBlock);
    }
}
