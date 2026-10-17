//! Items: source files, use declarations, declarations and their members
//! (02-grammar.md: Source Files, Test Blocks, Declarations, Generic
//! Parameters, Use Declarations, Annotations).

use hd_diag::Code;

use super::{Ctx, Marker, Parser, REST_ELSEWHERE, TRAIT_METHOD_VISIBILITY};
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

/// own.lifetime-param.plain
const LIFETIME_PARAMETER_PLAIN: &str =
    "a lifetime parameter takes no variance marker, bound, or default";

/// own.bound.one-mark
const BOUND_ONE_MARK: &str = "a bound list takes 'own' or 'mut', not both";

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
            self.bound_keywords.clear();
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
            let old = match self.nth_text(0) {
                "import" => Some((Code::OldImportDeclaration, "'import' was replaced by 'use'")),
                "export" => Some((
                    Code::OldExportDeclaration,
                    "'export' was replaced by 'pub use'",
                )),
                "struct" => Some((
                    Code::OldStructDeclaration,
                    "'struct' was replaced by 'data'",
                )),
                _ => None,
            };
            if let Some((code, message)) = old {
                self.error(code, message);
                self.recover_line();
                return;
            }
        }
        if self.at(TokenKind::KwTests) {
            if place == Place::Top {
                self.tests_block();
            } else {
                self.error(
                    Code::SyntaxError,
                    "a 'tests:' block may appear only at module top level",
                );
                self.recover_line();
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
        if public && place == Place::Body {
            // names.local.no-pub, fn.local.private
            let at = self.pos() - 1;
            self.error_at(
                Code::SyntaxError,
                at,
                "'pub' is not accepted on local declarations",
            );
        }
        match self.current() {
            Some(TokenKind::KwFn) => self.fn_decl(marker, Owner::Free),
            Some(TokenKind::KwData) => self.data_decl(marker),
            Some(TokenKind::KwEnum) => self.enum_decl(marker),
            Some(TokenKind::KwTrait) => self.trait_decl(marker),
            Some(TokenKind::KwImpl) => {
                if public {
                    // grammar.decl.impl-no-pub
                    let at = self.pos() - 1;
                    self.error_at(Code::SyntaxError, at, "'pub' is not accepted before 'impl'");
                }
                self.impl_decl(marker);
            }
            Some(TokenKind::KwType) => self.type_decl(marker, decorated),
            _ => {
                self.expected("a declaration");
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
                self.error_at(
                    Code::DecoratorNotTopLevel,
                    at,
                    "decorators are only valid on top-level declarations",
                );
            }
            let marker = self.start();
            self.bump();
            self.expr(Ctx::Closed);
            self.complete(marker, SyntaxKind::Decorator);
            if !self.eat_layout(Layout::Newline) {
                self.expected("a line ending after a decorator");
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
            if self.expect(TokenKind::Dot, "'.'") {
                self.expect_name("a dependency name");
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
            if !self.expect_name("a module path component") {
                break;
            }
        }
        if !grouped && self.at_word("as") {
            self.bump();
            self.expect_name("an import alias");
        }
        if public && !grouped {
            // grammar: `pub use` takes a group.
            let at = self.pos().saturating_sub(1);
            self.error_at(
                Code::SyntaxError,
                at,
                "only the grouped use form accepts 'pub'; write 'pub use module.{Name}'",
            );
        }
        self.complete(marker, SyntaxKind::UseDecl);
    }

    fn use_group(&mut self) {
        let marker = self.start();
        self.bump();
        while !self.at(TokenKind::RBrace) && self.current().is_some() {
            let item = self.start();
            if !self.expect_name("an imported declaration name") {
                self.abandon(item);
                break;
            }
            if self.at(TokenKind::Dot) {
                let at = self.pos();
                self.error_at(
                    Code::DirectVariantUse,
                    at,
                    "enum variants cannot be imported directly",
                );
                while self.eat(TokenKind::Dot) {
                    self.eat_name();
                }
            }
            if self.at_word("as") {
                self.bump();
                self.expect_name("an import alias");
            }
            self.complete(item, SyntaxKind::UseItem);
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RBrace, "'}'");
        self.complete(marker, SyntaxKind::UseGroup);
    }

    // ---------------------------------------------------------- functions

    /// `fn name[!] [generics] (params) [-> result] [$ row] (: suite | NEWLINE)`.
    pub(crate) fn fn_decl(&mut self, marker: Marker, owner: Owner) {
        self.bump(); // fn
        self.expect_name("a function name");
        if self.at(TokenKind::Bang) && self.nth_glued(0) {
            self.bump();
        }
        if self.at(TokenKind::LBracket) {
            self.generic_params(Generics::Function);
        }
        if self.at(TokenKind::LParen) {
            self.param_list(Params::Function);
        } else {
            self.expected("'(' after the function name");
        }
        if self.eat(TokenKind::Arrow) {
            self.ty_result();
        }
        if self.at(TokenKind::Dollar) {
            self.requirement_clause();
            if self.at(TokenKind::Comma) {
                // grammar.type.row.old-separator
                let at = self.pos();
                self.error_at(
                    Code::OldRowSeparator,
                    at,
                    "requirement keys are joined with '+': write '$ A + B'",
                );
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
            self.expected("':' or a line ending after the function signature");
            self.recover_line();
        } else if owner == Owner::Free {
            // A free function needs a body.
            self.expected("':' and a function body");
        }
        self.complete(marker, SyntaxKind::FnDecl);
    }

    /// `[ param, ... ]` of a declaration.
    pub(crate) fn generic_params(&mut self, mode: Generics) {
        let marker = self.start();
        self.bump(); // [
        let mut saw_lifetime = false;
        while !self.at(TokenKind::RBracket) && self.current().is_some() {
            let before = self.pos();
            let param = self.start();
            let variance = matches!(self.current(), Some(TokenKind::Plus | TokenKind::Minus))
                && mode == Generics::Type;
            if variance {
                self.bump();
            }
            if self.at(TokenKind::Caret) {
                self.lifetime_parameter(variance);
                saw_lifetime = true;
                self.complete(param, SyntaxKind::LifetimeParameter);
                if !self.eat(TokenKind::Comma) {
                    if !self.at(TokenKind::RBracket) {
                        self.expected("',' or ']'");
                        self.skip_to_list_end(TokenKind::RBracket);
                    }
                    break;
                }
                continue;
            }
            if saw_lifetime {
                // own.lifetime-param.last
                self.error(
                    Code::SyntaxError,
                    "lifetime parameters follow every type parameter",
                );
            }
            let row = self.eat(TokenKind::Dollar);
            self.expect_name("a generic parameter name");
            if self.at(TokenKind::Lt) {
                if row {
                    // grammar.generic.row-marker.no-bound
                    self.error(Code::SyntaxError, "a row parameter takes no bound");
                }
                self.bound_list();
            } else if self.at(TokenKind::Colon) {
                // grammar.decl.bound-vs-colon
                self.error(
                    Code::SyntaxError,
                    "a generic parameter bound is written with '<', not ':'",
                );
                self.bump();
                self.ty();
            }
            if self.at(TokenKind::Eq) {
                if mode == Generics::Impl {
                    // grammar.generic.default.impl
                    self.error(
                        Code::SyntaxError,
                        "generic parameters of an implementation take no default",
                    );
                }
                let default = self.start();
                self.bump();
                self.ty_argument();
                self.complete(default, SyntaxKind::TypeDefault);
            }
            self.complete(param, SyntaxKind::GenericParameter);
            if !self.eat(TokenKind::Comma) {
                if !self.at(TokenKind::RBracket) {
                    self.expected("',' or ']'");
                    self.skip_to_list_end(TokenKind::RBracket);
                }
                break;
            }
            if self.pos() == before {
                break;
            }
        }
        self.expect(TokenKind::RBracket, "']'");
        self.complete(marker, SyntaxKind::GenericParameterList);
    }

    /// `^name` in a generic parameter list (own.lifetime-param.declared-by):
    /// no variance, bound, or default.
    fn lifetime_parameter(&mut self, variance: bool) {
        if variance {
            // own.lifetime-param.plain
            self.error(Code::SyntaxError, LIFETIME_PARAMETER_PLAIN);
        }
        self.bump(); // ^
        self.expect_name("a lifetime parameter name");
        if self.at(TokenKind::Lt) {
            // own.lifetime-param.plain
            self.error(Code::SyntaxError, LIFETIME_PARAMETER_PLAIN);
            self.bound_list();
        }
        if self.at(TokenKind::Eq) {
            // own.lifetime-param.plain
            self.error(Code::SyntaxError, LIFETIME_PARAMETER_PLAIN);
            self.bump();
            self.ty_argument();
        }
    }

    /// `own` at the start of a bound list, before a trait name
    /// (own.bound.own).
    fn at_own_bound(&mut self) -> bool {
        self.at_word("own")
            && !self.nth_line_first(1)
            && matches!(
                self.nth(1),
                Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwMut)
            )
    }

    /// `< [mut | own] Bound & Bound ...` as a `BoundList`.
    pub(crate) fn bound_list(&mut self) {
        let marker = self.start();
        self.bump(); // <
        let mutable = self.eat(TokenKind::KwMut);
        if self.at_own_bound() {
            if mutable {
                // own.bound.one-mark
                self.error(Code::SyntaxError, BOUND_ONE_MARK);
            }
            self.token_node(SyntaxKind::ModeMark);
            if self.at(TokenKind::KwMut) {
                // own.bound.one-mark
                self.error(Code::SyntaxError, BOUND_ONE_MARK);
                self.bump();
            }
        }
        self.ty_bound_trait(true);
        loop {
            if self.eat(TokenKind::Amp) {
                self.ty_bound_trait(true);
            } else if self.at(TokenKind::Plus) {
                // grammar.generic.bound.old-plus
                let at = self.pos();
                self.error_at(
                    Code::OldBoundOperator,
                    at,
                    "several bounds are joined with '&': write 'A & B'",
                );
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

    /// `own self`, `own mut self`, `^self`, or `^mut self` ahead.
    fn at_receiver_mark(&mut self) -> bool {
        let first = self.at(TokenKind::Caret) || self.at_word("own");
        first
            && match self.nth(1) {
                Some(TokenKind::KwSelfValue) => true,
                Some(TokenKind::KwMut) => self.nth(2) == Some(TokenKind::KwSelfValue),
                _ => false,
            }
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
                    self.expected("',' or ')'");
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
                } else if self.at_receiver_mark() {
                    // own.receiver.forms
                    let mark = self.start();
                    if self.at(TokenKind::Caret) {
                        self.bump();
                        self.eat(TokenKind::KwMut);
                    } else {
                        self.bump(); // own
                        if self.at(TokenKind::KwMut) {
                            // own.syntax.no-own-mut
                            self.error(
                                Code::SyntaxError,
                                "'own' already grants mutation: write 'own self', not 'own mut self'",
                            );
                            self.bump();
                        }
                    }
                    self.complete(mark, SyntaxKind::ModeMark);
                    self.bump(); // self
                } else if self.at(TokenKind::KwMut) {
                    self.bump();
                    if !self.eat(TokenKind::KwSelfValue) {
                        // grammar: only the receiver takes `mut`.
                        let at = self.pos() - 1;
                        self.error_at(Code::SyntaxError, at, "'mut' goes on the parameter's type, not its name; only the receiver is 'mut self'");
                        self.eat_name();
                        if self.eat(TokenKind::Colon) {
                            self.ty();
                        }
                    }
                } else {
                    self.expect_name("a parameter name");
                    let vararg = self.eat(TokenKind::Ellipsis);
                    if self.expect(TokenKind::Colon, "':'") {
                        self.ty();
                    }
                    if self.at(TokenKind::Ellipsis) {
                        // grammar.type.rest.elsewhere
                        self.error(Code::SyntaxError, REST_ELSEWHERE);
                        self.bump();
                    }
                    if self.at(TokenKind::Eq) {
                        if vararg {
                            // grammar.fn.vararg-name
                            self.error(Code::SyntaxError, "a vararg parameter takes no default");
                        }
                        self.default_value(Ctx::Expr);
                    }
                }
            }
            Params::Closure => {
                self.expect_name("a closure parameter name");
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
                        self.error(
                            Code::SyntaxError,
                            "only shared enum constructor parameters declare defaults",
                        );
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
        self.expect_name("a data type name");
        if self.at(TokenKind::LBracket) {
            self.generic_params(Generics::Type);
        }
        if self.expect(TokenKind::Colon, "':'") {
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
            let name = 1 + usize::from(self.nth(1) == Some(TokenKind::KwPub));
            let field = matches!(self.nth(name), Some(TokenKind::Ident | TokenKind::RawIdent))
                && self.nth(name + 1) == Some(TokenKind::Colon);
            let message = if field {
                "data fields express mutable access in their type rather than with a field modifier"
            } else {
                "an embedded field takes no 'mut': access to an embedded part follows its container"
            };
            self.error_at(Code::MutableFieldModifier, at, message);
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
            self.error_at(Code::SyntaxError, at, "an embedded field takes no 'pub'");
        }
        if matches!(self.current(), Some(TokenKind::Ident)) {
            self.ty();
            self.complete(marker, SyntaxKind::EmbeddedField);
            return;
        }
        self.expected("a data field name");
        self.recover_line();
        self.complete(marker, SyntaxKind::Error);
    }

    fn enum_decl(&mut self, marker: Marker) {
        self.bump(); // enum
        self.expect_name("an enum type name");
        if self.at(TokenKind::LBracket) {
            self.generic_params(Generics::Type);
        }
        if self.at(TokenKind::LParen) {
            self.param_list(Params::Enum);
        }
        if self.expect(TokenKind::Colon, "':'") {
            self.member_block(false, Self::enum_variant);
        }
        self.complete(marker, SyntaxKind::EnumDecl);
    }

    fn enum_variant(&mut self) {
        self.attach_docs();
        let marker = self.start();
        self.decorators(false);
        self.attach_docs();
        self.expect_name("an enum variant name");
        if self.at(TokenKind::LBracket) {
            // grammar.enum.no-type-params
            let at = self.pos();
            self.error_at(
                Code::VariantResultTypeRemoved,
                at,
                "enum variants take no type parameters",
            );
            self.generic_params(Generics::Function);
        }
        if self.at(TokenKind::LParen) {
            self.param_list(Params::Variant);
        }
        if self.eat(TokenKind::Arrow) {
            let shared = self.start();
            let at = self.pos();
            self.expect_name("the enum name after '->'");
            if self.at(TokenKind::LBracket) {
                // grammar.enum.no-result-type
                self.error_at(
                    Code::VariantResultTypeRemoved,
                    at,
                    "a variant result names its enum and gives arguments, not a result type",
                );
                self.ty_arguments(true);
            }
            if self.at(TokenKind::LParen) {
                self.arguments();
            } else {
                self.error_at(
                    Code::VariantResultTypeRemoved,
                    at,
                    "a variant result names its enum and gives an argument clause",
                );
            }
            self.complete(shared, SyntaxKind::VariantSharedData);
        }
        if self.at(TokenKind::Colon) {
            // A variant takes no block.
            self.expected("a line ending after an enum variant");
            self.recover_line();
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
                    self.expected(if pass_inline {
                        "a line ending or 'pass' after ':'"
                    } else {
                        "a line ending after ':'"
                    });
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
            SuiteKind::Missing => self.error(
                Code::SyntaxError,
                "expected an indented block, found a line that is not indented deeper",
            ),
        }
        self.suite_closed = true;
        self.complete(block, SyntaxKind::Block);
    }

    // -------------------------------------------------------- trait, impl

    fn trait_decl(&mut self, marker: Marker) {
        self.bump(); // trait
        self.expect_name("a trait name");
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
                self.expected("a line ending after the trait header");
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
            self.error_at(Code::TraitMethodVisibility, at, TRAIT_METHOD_VISIBILITY);
            self.bump();
        }
        if self.at(TokenKind::KwFn) {
            self.fn_decl(marker, Owner::Trait);
        } else {
            self.expected("'fn' or 'type'");
            self.recover_line();
            self.complete(marker, SyntaxKind::Error);
        }
    }

    fn associated_type(&mut self, with_value: bool) {
        let marker = self.start();
        self.bump(); // type
        self.expect_name("an associated type name");
        if self.at(TokenKind::Eq) {
            if !with_value {
                self.error(
                    Code::SyntaxError,
                    "an associated type in a trait declares no value",
                );
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
            self.expect_name("an embedded field name");
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
                self.error_at(Code::TraitMethodVisibility, at, TRAIT_METHOD_VISIBILITY);
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
        self.expected("a method, an associated type, or a derivation");
        self.recover_line();
        self.complete(marker, SyntaxKind::Error);
    }

    // --------------------------------------------------------- type, tests

    fn type_decl(&mut self, marker: Marker, decorated: bool) {
        self.bump(); // type
        self.expect_name("a type name");
        if self.at(TokenKind::LBracket) {
            self.generic_params(Generics::Type);
        }
        if self.at(TokenKind::LParen) {
            self.bump();
            self.ty();
            self.expect(TokenKind::RParen, "')'");
        } else if self.eat(TokenKind::Eq) {
            if decorated {
                // grammar.annot.alias-no-decorator
                let at = self.pos() - 1;
                self.error_at(
                    Code::SyntaxError,
                    at,
                    "a transparent alias takes no decorator",
                );
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
            self.expected("'=' or '('");
        }
        self.complete(marker, SyntaxKind::TypeDecl);
    }

    fn tests_block(&mut self) {
        let marker = self.start();
        self.bump(); // tests
        if self.expect(TokenKind::Colon, "':'") {
            self.member_block(false, |p| {
                if p.at(TokenKind::KwTests) {
                    // grammar.tests.top-level
                    p.error(
                        Code::SyntaxError,
                        "a 'tests:' block may appear only at module top level",
                    );
                    p.recover_line();
                } else {
                    p.item(Place::Tests);
                }
            });
        }
        self.complete(marker, SyntaxKind::TestsBlock);
    }
}
