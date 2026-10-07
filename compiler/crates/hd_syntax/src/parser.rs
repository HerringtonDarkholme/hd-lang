use std::collections::HashMap;

use hd_base::{FileId, ItemIdx, NodeIdx, Span, Symbol, TokenIdx};
use hd_diag::{Code, Diagnostic};
use hd_intern::Interner;

use crate::green::{Event, build};
use crate::{CommentKind, GreenTree, Layout, SyntaxKind, TokenBuf, TokenKind, lex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ItemKind {
    Function,
    Data,
    Enum,
    Trait,
    Impl,
    Alias,
    Test,
    Statement,
}

#[derive(Clone, Debug, Default)]
pub struct ItemIndex {
    pub at: Vec<NodeIdx>,
    pub parent: Vec<ItemIdx>,
    pub name: Vec<Symbol>,
    pub kind: Vec<ItemKind>,
    pub flags: Vec<u8>,
    pub by_name: HashMap<Symbol, ItemIdx>,
}

#[derive(Clone, Debug)]
pub struct Parse {
    pub tokens: TokenBuf,
    pub tree: GreenTree,
    pub diagnostics: Vec<Diagnostic>,
    pub items: ItemIndex,
    pub symbols: Interner,
}

impl Parse {
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.diagnostics.is_empty()
    }

    #[must_use]
    pub fn diagnostic_codes(&self) -> Vec<Code> {
        self.diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }
}

#[must_use]
pub fn parse(source: &[u8]) -> Parse {
    let lexed = lex(source);
    let text = core::str::from_utf8(source).unwrap_or_default();
    let mut parser = Parser::new(text, &lexed.tokens);
    parser.source_file();
    let layouts = collect_layout(&lexed.tokens);
    let tree = build(&parser.events, &layouts, &[]);
    let (items, symbols) = build_item_index(&tree, &lexed.tokens, text);
    let mut diagnostics = lexed.diagnostics;
    diagnostics.extend(validate(text, &lexed.tokens));
    deduplicate(&mut diagnostics);
    Parse {
        tokens: lexed.tokens,
        tree,
        diagnostics,
        items,
        symbols,
    }
}

struct Parser<'t> {
    source: &'t str,
    tokens: &'t TokenBuf,
    line_end: Vec<usize>,
    events: Vec<Event>,
}

impl<'t> Parser<'t> {
    fn new(source: &'t str, tokens: &'t TokenBuf) -> Self {
        Self {
            source,
            tokens,
            line_end: tokens.line_token_ends(),
            events: Vec::with_capacity(tokens.len() * 2),
        }
    }

    fn source_file(&mut self) {
        self.events.push(Event::Start(SyntaxKind::Root));
        self.parse_lines(0, self.tokens.line_start.len(), 0, true, SyntaxKind::Root);
        self.events.push(Event::Finish);
    }

    /// Nests one line's tokens (`structure.rs`) where the line kind has a
    /// grammar there; other lines keep their flat tokens.
    fn emit_line(&mut self, first: usize, end: usize, kind: SyntaxKind, container: SyntaxKind) {
        let structured = match kind {
            SyntaxKind::FnDecl | SyntaxKind::ImplDecl | SyntaxKind::DataField => true,
            SyntaxKind::Statement => !matches!(
                container,
                SyntaxKind::EnumDecl | SyntaxKind::TraitDecl | SyntaxKind::Root
            ),
            _ => false,
        };
        if !structured {
            let statement =
                (kind == SyntaxKind::Statement).then(|| self.statement_kind(first, end));
            if let Some(statement) = statement {
                self.events.push(Event::Start(statement));
            }
            self.emit_tokens(first, end, kind);
            if statement.is_some() {
                self.events.push(Event::Finish);
            }
            return;
        }
        let mut line = crate::structure::Line::new(self.tokens, first, end, &mut self.events);
        match kind {
            SyntaxKind::FnDecl => line.fn_header(),
            SyntaxKind::ImplDecl => line.impl_header(),
            SyntaxKind::DataField => line.data_field(),
            _ => line.statement(),
        }
    }

    fn parse_lines(
        &mut self,
        mut line: usize,
        end_line: usize,
        indent: u16,
        top_level: bool,
        container: SyntaxKind,
    ) -> usize {
        while line < end_line {
            let Some((first, end)) = self.line_tokens(line) else {
                line += 1;
                continue;
            };
            let line_indent = self.tokens.line_indent[line];
            if line_indent < indent {
                break;
            }
            if line_indent > indent && !top_level {
                break;
            }
            let mut kind = self.line_kind(first, end, top_level);
            if kind == SyntaxKind::Statement && container == SyntaxKind::DataDecl {
                kind = SyntaxKind::DataField;
            }
            self.events.push(Event::Start(kind));
            self.emit_line(first, end, kind, container);
            if self.has_indented_body(line, end) {
                let Some(body_line) = self.next_token_line(line + 1) else {
                    self.events.push(Event::Finish);
                    return end_line;
                };
                let body_indent = self.tokens.line_indent[body_line];
                self.events.push(Event::Start(SyntaxKind::Block));
                let inner = if kind == SyntaxKind::Statement {
                    container
                } else {
                    kind
                };
                line = self.parse_lines(body_line, end_line, body_indent, false, inner);
                self.events.push(Event::Finish);
            } else {
                line += 1;
            }
            self.events.push(Event::Finish);
        }
        line
    }

    fn emit_tokens(&mut self, mut first: usize, end: usize, _parent: SyntaxKind) {
        while first < end {
            let kind = match self.tokens.kind[first] {
                TokenKind::KwDyn => Some(SyntaxKind::DynType),
                TokenKind::String => Some(SyntaxKind::StringExpr),
                TokenKind::KwIf => Some(SyntaxKind::IfExpr),
                TokenKind::KwFor => Some(SyntaxKind::ForExpr),
                TokenKind::KwWhile => Some(SyntaxKind::WhileExpr),
                TokenKind::KwMatch => Some(SyntaxKind::MatchExpr),
                TokenKind::PipeGt => Some(SyntaxKind::PipeExpr),
                TokenKind::DotDot | TokenKind::DotDotEq => Some(SyntaxKind::RangeExpr),
                TokenKind::Dollar => Some(SyntaxKind::RequirementRow),
                _ => None,
            };
            if let Some(kind) = kind {
                self.events.push(Event::Start(kind));
                if kind == SyntaxKind::StringExpr
                    && self
                        .tokens
                        .text(TokenIdx::from_raw(as_u32(first)), self.source)
                        .contains('$')
                {
                    self.events.push(Event::Start(SyntaxKind::Interpolation));
                    self.token(first);
                    self.events.push(Event::Finish);
                } else {
                    self.token(first);
                }
                self.events.push(Event::Finish);
            } else {
                self.token(first);
            }
            first += 1;
        }
    }

    fn token(&mut self, index: usize) {
        self.events
            .push(Event::Token(TokenIdx::from_raw(as_u32(index))));
    }

    fn statement_kind(&self, first: usize, end: usize) -> SyntaxKind {
        let kinds = &self.tokens.kind[first..end];
        match kinds.first().copied() {
            Some(TokenKind::KwLet) => SyntaxKind::LetStmt,
            Some(TokenKind::Placeholder) if kinds.get(1) == Some(&TokenKind::ColonEq) => {
                SyntaxKind::DiscardStmt
            }
            Some(TokenKind::KwReturn) => SyntaxKind::ReturnStmt,
            Some(TokenKind::KwBreak) => SyntaxKind::BreakStmt,
            Some(TokenKind::KwContinue) => SyntaxKind::ContinueStmt,
            Some(TokenKind::KwDefer) => SyntaxKind::DeferStmt,
            _ if kinds.contains(&TokenKind::FatArrow) => SyntaxKind::MatchArm,
            _ if kinds.iter().any(|kind| {
                matches!(
                    kind,
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
                        | TokenKind::ShrEq
                )
            }) =>
            {
                SyntaxKind::AssignmentStmt
            }
            _ => SyntaxKind::ExprStmt,
        }
    }

    fn line_kind(&self, first: usize, end: usize, top_level: bool) -> SyntaxKind {
        let mut at = first;
        if self.tokens.kind.get(at) == Some(&TokenKind::KwPub) {
            at += 1;
        }
        if self.tokens.kind.get(at) == Some(&TokenKind::At) {
            return SyntaxKind::Decorator;
        }
        match self.tokens.kind.get(at).copied() {
            Some(TokenKind::KwFn) => SyntaxKind::FnDecl,
            Some(TokenKind::KwData) => SyntaxKind::DataDecl,
            Some(TokenKind::KwEnum) => SyntaxKind::EnumDecl,
            Some(TokenKind::KwTrait) => SyntaxKind::TraitDecl,
            Some(TokenKind::KwImpl) => SyntaxKind::ImplDecl,
            Some(TokenKind::KwType) => SyntaxKind::TypeDecl,
            Some(TokenKind::KwTests) => SyntaxKind::TestsBlock,
            Some(TokenKind::Ident)
                if top_level
                    && self
                        .tokens
                        .text(TokenIdx::from_raw(as_u32(at)), self.source)
                        == "use" =>
            {
                SyntaxKind::UseDecl
            }
            _ if end > first => SyntaxKind::Statement,
            _ => SyntaxKind::Error,
        }
    }

    fn line_tokens(&self, line: usize) -> Option<(usize, usize)> {
        let first = self.tokens.line_tok[line].get()?.idx();
        Some((first, self.line_end[line]))
    }

    fn next_token_line(&self, from: usize) -> Option<usize> {
        (from..self.tokens.line_tok.len()).find(|&line| self.tokens.line_tok[line].get().is_some())
    }

    fn has_indented_body(&self, line: usize, end: usize) -> bool {
        let Some((first, _)) = self.line_tokens(line) else {
            return false;
        };
        if !self.tokens.kind[first..end].contains(&TokenKind::Colon) {
            return false;
        }
        self.next_token_line(line + 1)
            .is_some_and(|next| self.tokens.line_indent[next] > self.tokens.line_indent[line])
    }
}

fn collect_layout(tokens: &TokenBuf) -> Vec<(TokenIdx, Layout)> {
    let mut result = Vec::new();
    let mut indents = vec![0_u16];
    let mut first_line = true;
    let line_end = tokens.line_token_ends();
    for (line, &end) in line_end.iter().enumerate() {
        let Some(first) = tokens.line_tok[line].get() else {
            continue;
        };
        if !first_line {
            result.push((first, Layout::Newline));
        }
        first_line = false;
        let indent = tokens.line_indent[line];
        if indent > *indents.last().expect("indent stack") {
            indents.push(indent);
            result.push((first, Layout::Indent));
        } else {
            while indents.last().is_some_and(|&active| active > indent) {
                indents.pop();
                result.push((first, Layout::Dedent));
            }
        }
        if let Some(colon) =
            (first.idx()..end).rfind(|&index| tokens.kind[index] == TokenKind::Colon)
            && colon + 1 < end
        {
            result.push((TokenIdx::from_raw(as_u32(end)), Layout::SuiteEnd));
        }
    }
    let eof = TokenIdx::from_raw(as_u32(tokens.len()));
    if !first_line {
        result.push((eof, Layout::Newline));
    }
    while indents.len() > 1 {
        indents.pop();
        result.push((eof, Layout::Dedent));
    }
    result
}

fn build_item_index(tree: &GreenTree, tokens: &TokenBuf, source: &str) -> (ItemIndex, Interner) {
    let mut result = ItemIndex::default();
    let mut symbols = Interner::new();
    for child in tree.root().children() {
        let kind = match child.kind() {
            SyntaxKind::FnDecl => ItemKind::Function,
            SyntaxKind::DataDecl => ItemKind::Data,
            SyntaxKind::EnumDecl => ItemKind::Enum,
            SyntaxKind::TraitDecl => ItemKind::Trait,
            SyntaxKind::ImplDecl => ItemKind::Impl,
            SyntaxKind::TypeDecl => ItemKind::Alias,
            SyntaxKind::TestsBlock => ItemKind::Test,
            SyntaxKind::UseDecl | SyntaxKind::Decorator => continue,
            _ => ItemKind::Statement,
        };
        let name = item_name(child.index(), tree, tokens, source)
            .map_or(Symbol::NONE, |text| symbols.intern(text));
        let item = ItemIdx::from_raw(as_u32(result.at.len()));
        result.at.push(child.index());
        result.parent.push(ItemIdx::NONE);
        result.name.push(name);
        result.kind.push(kind);
        result.flags.push(0);
        if name.get().is_some() {
            result.by_name.entry(name).or_insert(item);
        }
    }
    (result, symbols)
}

fn item_name<'s>(
    node: NodeIdx,
    tree: &GreenTree,
    tokens: &TokenBuf,
    source: &'s str,
) -> Option<&'s str> {
    let (first, last) = tree.span_tokens(node);
    let mut saw_head = false;
    for raw in first.raw()..=last.raw() {
        let token = TokenIdx::from_raw(raw);
        let kind = tokens.kind(token);
        if matches!(
            kind,
            TokenKind::KwFn
                | TokenKind::KwData
                | TokenKind::KwEnum
                | TokenKind::KwTrait
                | TokenKind::KwType
        ) {
            saw_head = true;
        } else if saw_head && matches!(kind, TokenKind::Ident | TokenKind::RawIdent) {
            return Some(tokens.text(token, source).trim_matches('`'));
        }
    }
    None
}

fn validate(source: &str, tokens: &TokenBuf) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    validate_delimiters(tokens, &mut diagnostics);
    validate_indentation(tokens, &mut diagnostics);
    validate_continuations(source, tokens, &mut diagnostics);
    validate_comments(source, tokens, &mut diagnostics);
    let mut enum_context: Option<(u16, bool)> = None;
    let mut container = None;
    let line_end = tokens.line_token_ends();
    for (line, &end) in line_end.iter().enumerate() {
        let Some(first) = tokens.line_tok[line].get() else {
            continue;
        };
        let start = first.idx();
        let kinds = &tokens.kind[start..end];
        let word = tokens.text(first, source);
        let indent = tokens.line_indent[line];
        if container.is_some_and(|(header_indent, _)| indent <= header_indent) {
            container = None;
        }
        if enum_context.is_some_and(|(header_indent, _)| indent <= header_indent) {
            enum_context = None;
        }
        let variant_has_type_parameters = kinds
            .iter()
            .position(|kind| *kind == TokenKind::LBracket)
            .is_some_and(|bracket| {
                kinds
                    .iter()
                    .position(|kind| *kind == TokenKind::LParen)
                    .is_none_or(|payload| bracket < payload)
            });
        if enum_context.is_some_and(|(_, shared)| !shared)
            && (kinds.contains(&TokenKind::Arrow) || variant_has_type_parameters)
        {
            push_diag(
                &mut diagnostics,
                Code::VariantResultTypeRemoved,
                tokens.start[start],
                tokens.end[end - 1],
            );
        }
        if kinds.first() == Some(&TokenKind::KwEnum) {
            let shared = kinds.contains(&TokenKind::LParen);
            enum_context = Some((indent, shared));
        }
        if kinds.first() == Some(&TokenKind::At)
            && container.is_some_and(|(_, kind)| kind == TokenKind::KwFn)
        {
            push_diag(
                &mut diagnostics,
                Code::DecoratorNotTopLevel,
                tokens.start[start],
                tokens.end[end - 1],
            );
        }
        let head = usize::from(kinds.first() == Some(&TokenKind::KwPub));
        let previous_container = container.map(|(_, kind)| kind);
        if kinds.starts_with(&[TokenKind::KwPub, TokenKind::KwFn])
            && previous_container == Some(TokenKind::KwTrait)
        {
            push_diag(
                &mut diagnostics,
                Code::TraitMethodVisibility,
                tokens.start[start],
                tokens.end[end - 1],
            );
        }
        if word == "import" {
            push_diag(
                &mut diagnostics,
                Code::OldImportDeclaration,
                tokens.start[start],
                tokens.end[start],
            );
        }
        if word == "export" {
            push_diag(
                &mut diagnostics,
                Code::OldExportDeclaration,
                tokens.start[start],
                tokens.end[start],
            );
        }
        if word == "struct" {
            push_diag(
                &mut diagnostics,
                Code::OldStructDeclaration,
                tokens.start[start],
                tokens.end[start],
            );
        }
        if kinds.starts_with(&[TokenKind::KwPub])
            && kinds
                .iter()
                .any(|kind| matches!(kind, TokenKind::ColonEq | TokenKind::Eq))
        {
            push_diag(
                &mut diagnostics,
                Code::SyntaxError,
                tokens.start[start],
                tokens.end[end - 1],
            );
        }
        let structural_edge = kinds.first() == Some(&TokenKind::Colon)
            || kinds
                .windows(2)
                .any(|pair| pair == [TokenKind::Dot, TokenKind::Number])
            || kinds
                .iter()
                .filter(|kind| **kind == TokenKind::Colon)
                .count()
                > 1
            || kinds.first() == Some(&TokenKind::Ident)
                && kinds.contains(&TokenKind::LBracket)
                && kinds
                    .iter()
                    .any(|kind| matches!(kind, TokenKind::LBrace | TokenKind::ColonColon))
            || kinds.contains(&TokenKind::Eq) && kinds.contains(&TokenKind::Comma)
            || word == "i32" && kinds.contains(&TokenKind::Eq);
        if word == "use"
            || structural_edge
            || needs_line_validation(kinds, previous_container, enum_context.is_some())
        {
            validate_line(
                source,
                tokens,
                start,
                end,
                previous_container,
                enum_context.is_some(),
                &mut diagnostics,
            );
        }
        if matches!(
            kinds.get(head),
            Some(
                TokenKind::KwTrait
                    | TokenKind::KwImpl
                    | TokenKind::KwFn
                    | TokenKind::KwData
                    | TokenKind::KwEnum
                    | TokenKind::KwTests
            )
        ) && kinds.contains(&TokenKind::Colon)
        {
            container = Some((indent, kinds[head]));
        }
    }
    diagnostics
}

fn needs_line_validation(kinds: &[TokenKind], container: Option<TokenKind>, in_enum: bool) -> bool {
    in_enum
        || container.is_some()
            && kinds.iter().any(|kind| {
                matches!(
                    kind,
                    TokenKind::KwFn | TokenKind::KwMut | TokenKind::KwTests | TokenKind::Eq
                )
            })
        || kinds.iter().any(|kind| {
            matches!(
                kind,
                TokenKind::String
                    | TokenKind::KwData
                    | TokenKind::KwDefer
                    | TokenKind::KwDyn
                    | TokenKind::KwEnum
                    | TokenKind::KwFn
                    | TokenKind::KwFor
                    | TokenKind::KwIf
                    | TokenKind::KwImpl
                    | TokenKind::KwLet
                    | TokenKind::KwMut
                    | TokenKind::KwPub
                    | TokenKind::KwTests
                    | TokenKind::KwTrait
                    | TokenKind::KwType
                    | TokenKind::DotDot
                    | TokenKind::DotDotEq
                    | TokenKind::Ellipsis
                    | TokenKind::EllipsisEq
                    | TokenKind::StarStar
                    | TokenKind::EqEq
                    | TokenKind::NotEq
                    | TokenKind::Lt
                    | TokenKind::LtEq
                    | TokenKind::Gt
                    | TokenKind::GtEq
                    | TokenKind::ColonEq
                    | TokenKind::ColonColon
                    | TokenKind::Question
                    | TokenKind::Dollar
                    | TokenKind::At
                    | TokenKind::FatArrow
            )
        })
}

fn comparison_chains(kinds: &[TokenKind]) -> bool {
    let mut depth = 0_usize;
    let mut counts = [0_u8; 32];
    for &kind in kinds {
        match kind {
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => {
                depth = (depth + 1).min(31);
            }
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                counts[depth] = 0;
                depth = depth.saturating_sub(1);
            }
            TokenKind::Comma
            | TokenKind::Colon
            | TokenKind::FatArrow
            | TokenKind::ColonEq
            | TokenKind::Eq
            | TokenKind::AndAnd
            | TokenKind::OrOr => counts[depth] = 0,
            TokenKind::EqEq
            | TokenKind::NotEq
            | TokenKind::Lt
            | TokenKind::LtEq
            | TokenKind::Gt
            | TokenKind::GtEq
            | TokenKind::KwIs => {
                counts[depth] += 1;
                if counts[depth] > 1 {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn mut_parameter(kinds: &[TokenKind]) -> bool {
    kinds.windows(4).any(|window| {
        matches!(window[0], TokenKind::LParen | TokenKind::Comma)
            && window[1] == TokenKind::KwMut
            && window[2] == TokenKind::Ident
            && window[3] == TokenKind::Colon
    })
}

fn validate_line(
    source: &str,
    tokens: &TokenBuf,
    start: usize,
    end: usize,
    container: Option<TokenKind>,
    in_enum: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if start >= end {
        return;
    }
    let lo = tokens.start[start];
    let hi = tokens.end[end - 1];
    let raw = source[lo as usize..hi as usize].trim();
    let kinds = &tokens.kind[start..end];
    let has = |kind| kinds.contains(&kind);
    let syntax = |output: &mut Vec<Diagnostic>| push_diag(output, Code::SyntaxError, lo, hi);

    if comparison_chains(kinds) {
        push_diag(diagnostics, Code::ComparisonChaining, lo, hi);
    }
    let bad_mut_parameter = mut_parameter(kinds);
    if bad_mut_parameter {
        syntax(diagnostics);
    }

    if raw.starts_with("mut ")
        && !raw.starts_with("mut self")
        && !raw.contains(":=")
        && (container == Some(TokenKind::KwData) && raw.contains(':')
            || !bad_mut_parameter && !raw.contains(':'))
    {
        push_diag(diagnostics, Code::MutableFieldModifier, lo, hi);
    }
    if raw.starts_with("pub ") && kinds.len() == 2 && kinds[1] == TokenKind::Ident {
        syntax(diagnostics);
    }
    if raw == ":" {
        push_diag(diagnostics, Code::TrailingBlockPosition, lo, hi);
    }
    if raw.starts_with(": ") {
        syntax(diagnostics);
    }
    if raw.starts_with("use ")
        && raw
            .split_once('{')
            .is_some_and(|(_, grouped)| grouped.contains('.'))
    {
        push_diag(diagnostics, Code::DirectVariantUse, lo, hi);
    }
    if named_before_positional(kinds) && !raw.contains("$.with(") && !raw.contains("$.context(") {
        push_diag(
            diagnostics,
            if kinds.contains(&TokenKind::FatArrow) {
                Code::PatternOrder
            } else {
                Code::ArgumentOrder
            },
            lo,
            hi,
        );
    }
    if (raw.contains(" and ") || raw.contains(" or ")) && !raw.contains("&&") && !raw.contains("||")
        || raw.starts_with("not ")
        || raw.contains(": not ")
    {
        syntax(diagnostics);
    }
    if old_bound_plus(kinds) {
        push_diag(diagnostics, Code::OldBoundOperator, lo, hi);
    }
    if old_row_comma(kinds) {
        push_diag(diagnostics, Code::OldRowSeparator, lo, hi);
    }
    if kinds
        .windows(2)
        .any(|pair| pair == [TokenKind::Dot, TokenKind::Number])
    {
        syntax(diagnostics);
    }
    if kinds
        .windows(2)
        .any(|pair| pair == [TokenKind::Dollar, TokenKind::KwMut])
    {
        syntax(diagnostics);
    }
    if kinds
        .windows(2)
        .any(|pair| pair == [TokenKind::KwDyn, TokenKind::KwMut])
    {
        syntax(diagnostics);
    }
    if kinds
        .windows(2)
        .any(|pair| pair == [TokenKind::StarStar, TokenKind::Eq])
    {
        syntax(diagnostics);
    }
    if kinds
        .windows(2)
        .any(|pair| pair == [TokenKind::KwTests, TokenKind::ColonEq])
    {
        syntax(diagnostics);
    }
    if kinds.iter().enumerate().any(|(index, kind)| {
        kind.is_keyword()
            && tokens.end[start + index]
                == tokens
                    .start
                    .get(start + index + 1)
                    .copied()
                    .unwrap_or(u32::MAX)
            && tokens.kind.get(start + index + 1) == Some(&TokenKind::String)
    }) {
        syntax(diagnostics);
    }
    if kinds.contains(&TokenKind::String) {
        for index in start..end {
            if tokens.kind[index] == TokenKind::String {
                let text = tokens.text(TokenIdx::from_raw(as_u32(index)), source);
                if ["$true", "$false", "$let", "$return", "$type", "$match"]
                    .iter()
                    .any(|word| text.contains(word))
                {
                    syntax(diagnostics);
                }
                if [
                    "if\"", "for\"", "let\"", "match\"", "return\"", "true\"", "false\"",
                ]
                .iter()
                .any(|prefix| text.starts_with(prefix))
                {
                    syntax(diagnostics);
                }
            }
        }
    }
    for index in start..end {
        if tokens.kind[index] == TokenKind::Number {
            let text = tokens.text(TokenIdx::from_raw(as_u32(index)), source);
            if ["else", "if", "for", "while", "match", "return", "let"]
                .iter()
                .any(|suffix| text.ends_with(suffix))
            {
                syntax(diagnostics);
            }
        }
    }

    let adjacent =
        |left: usize, right: usize| tokens.end[start + left] == tokens.start[start + right];
    for index in 0..kinds.len().saturating_sub(1) {
        if adjacent(index, index + 1)
            && ((kinds[index] == TokenKind::Number && kinds[index + 1].is_keyword())
                || (kinds[index].is_keyword() && kinds[index + 1] == TokenKind::String)
                || (kinds[index] == TokenKind::String && kinds[index + 1] == TokenKind::Ident))
        {
            syntax(diagnostics);
        }
        if index > 0
            && adjacent(index, index + 1)
            && kinds[index - 1] == TokenKind::Dot
            && kinds[index] == TokenKind::Ident
            && kinds[index + 1] == TokenKind::String
        {
            push_diag(diagnostics, Code::QualifiedStringPrefix, lo, hi);
        }
        if kinds[index] == TokenKind::Dot && kinds[index + 1] == TokenKind::String {
            push_diag(diagnostics, Code::QualifiedStringPrefix, lo, hi);
        }
    }

    if (has(TokenKind::Dollar)
        && has(TokenKind::Ellipsis)
        && (raw.contains("$.with(...") || raw.contains("$.context(...")))
        || (has(TokenKind::KwMut)
            && ((has(TokenKind::KwFn) && (raw.contains("-> mut fn") || raw.contains(":= mut fn")))
                || (has(TokenKind::Dollar) && raw.contains(" $ mut "))
                || (has(TokenKind::KwFor) && raw.contains("for mut "))))
        || (has(TokenKind::KwType) && has(TokenKind::Dollar) && raw.contains("type Both = $"))
        || (has(TokenKind::KwFn) && raw.contains("fn[T]"))
        || (has(TokenKind::Minus) && raw.contains(" - Logger"))
        || raw.contains("[reified ")
    {
        syntax(diagnostics);
    }
    if raw.contains("$(") && !raw.contains("$()")
        || raw.matches('$').count() > 1 && raw.contains("-> fn")
    {
        syntax(diagnostics);
    }
    if raw.contains("$.use(mut ") || raw.contains("$.with(mut ") || raw.contains("$.context(mut ") {
        syntax(diagnostics);
    }
    if has(TokenKind::KwImpl)
        && (start..end).any(|index| {
            matches!(tokens.kind[index], TokenKind::Ident | TokenKind::RawIdent)
                && tokens.text(TokenIdx::from_raw(as_u32(index)), source) == "where"
        })
    {
        syntax(diagnostics);
    }
    if kinds.first() == Some(&TokenKind::KwImpl)
        && between_first_pair(kinds, TokenKind::LBracket, TokenKind::RBracket)
            .is_some_and(|range| kinds[range].contains(&TokenKind::Eq))
    {
        syntax(diagnostics);
    }
    if kinds.first() == Some(&TokenKind::KwPub)
        && raw.starts_with("pub use ")
        && !raw.contains(".{")
    {
        syntax(diagnostics);
    }
    if kinds.first() == Some(&TokenKind::Ident)
        && raw.starts_with("use ")
        && !(raw.starts_with("use pkg.")
            || raw.starts_with("use std.")
            || raw.starts_with("use dep.")
            || raw.starts_with("use self.")
            || raw.starts_with("use super."))
    {
        syntax(diagnostics);
    }
    if kinds.first() == Some(&TokenKind::KwPub) && has(TokenKind::ColonEq) {
        syntax(diagnostics);
    }
    if (has(TokenKind::Dot) && has(TokenKind::Star) && raw.contains(".*"))
        || (has(TokenKind::StarStar) && has(TokenKind::Eq))
        || (has(TokenKind::Ellipsis) && (raw.contains(" = 0...") || raw.contains("... =")))
        || (has(TokenKind::ColonColon)
            && has(TokenKind::Bang)
            && raw.contains("::[")
            && raw.contains("]!("))
    {
        syntax(diagnostics);
    }
    if has(TokenKind::Ellipsis) && has(TokenKind::LBrace) && raw.contains(", ...") {
        syntax(diagnostics);
    }
    if has(TokenKind::Ellipsis)
        && (raw.contains("...:") && has(TokenKind::Eq)
            || raw.contains(": i32...")
            || raw.contains("[Ts...]")
            || raw.contains("(i32...)")
            || raw.contains(":= (values...)")
            || raw.contains("xs..., 1")
            || raw.contains("rest..., first"))
    {
        syntax(diagnostics);
    }
    if kinds.first() == Some(&TokenKind::KwFn) && raw.contains("[T:") {
        syntax(diagnostics);
    }
    if kinds.first() == Some(&TokenKind::KwTrait) && raw.contains(": ") && !raw.ends_with(':') {
        syntax(diagnostics);
    }
    if kinds.first() == Some(&TokenKind::Ident) && has(TokenKind::Eq) && raw.starts_with("i32 ") {
        syntax(diagnostics);
    }
    if (has(TokenKind::Char) && raw.contains("r'"))
        || (has(TokenKind::Dot)
            && (raw.contains(".type(")
                || raw.contains(".type ")
                || raw.strip_suffix(".type").is_some()))
        || (has(TokenKind::Question) && raw.contains("number?"))
    {
        syntax(diagnostics);
    }
    if has(TokenKind::KwIf) && raw.contains(": if ") && raw.matches(':').count() > 2 {
        syntax(diagnostics);
    }
    if kinds.first() == Some(&TokenKind::KwFn) && raw.contains(": if ") {
        syntax(diagnostics);
    }
    if has(TokenKind::KwDefer) && has(TokenKind::KwIf) && raw.contains("defer: if ") {
        syntax(diagnostics);
    }
    if has(TokenKind::KwImpl) && raw.contains("[Item =") {
        syntax(diagnostics);
    }
    if has(TokenKind::ColonColon) && raw.contains("Supplier::[Item =") {
        syntax(diagnostics);
    }
    if has(TokenKind::KwLet)
        && raw.starts_with("let ")
        && raw
            .split_once('=')
            .is_some_and(|(pattern, _)| pattern.contains(','))
        && !raw.contains("let (")
    {
        syntax(diagnostics);
    }
    if raw.starts_with("it(") && raw.contains(": let (") && raw.contains(',') {
        syntax(diagnostics);
    }
    if raw.starts_with("let (")
        && raw
            .find(')')
            .is_some_and(|close| !raw[..close].contains(','))
    {
        syntax(diagnostics);
    }
    if has(TokenKind::KwLet)
        && raw.contains(": let ")
        && raw
            .split_once('=')
            .is_some_and(|(pattern, _)| pattern.contains(','))
        && !raw.contains("let (")
    {
        syntax(diagnostics);
    }
    if has(TokenKind::KwFor)
        && raw.starts_with("for ")
        && raw[..raw.find(" in ").unwrap_or(raw.len())].contains(',')
        && !raw[..raw.find(" in ").unwrap_or(raw.len())].contains('{')
        && !raw.starts_with("for (")
    {
        syntax(diagnostics);
    }
    if has(TokenKind::KwFor)
        && raw.contains("[for ")
        && raw
            .split_once(" =>")
            .map_or(raw, |(head, _)| head)
            .contains(", ")
        && !raw.contains("[for (")
        && !raw
            .split_once(" =>")
            .map_or(raw, |(head, _)| head)
            .contains('{')
    {
        syntax(diagnostics);
    }
    if has(TokenKind::ColonEq)
        && raw.contains(" := ")
        && ((raw.starts_with('(')
            && raw
                .find(" := ")
                .is_some_and(|bind| raw.find(')').is_none_or(|close| close < bind)))
            || raw.starts_with('.')
            || (raw.chars().next().is_some_and(char::is_uppercase) && raw.contains(" { ")))
    {
        push_diag(diagnostics, Code::MissingLet, lo, hi);
    }
    let binding_list = raw.starts_with('(') && raw.contains(") :=")
        || raw.contains("[(") && raw.contains(") :=")
        || raw.contains("((") && raw.contains(") :=")
        || raw.split_once(":=").is_some_and(|(left, _)| {
            left.contains(',') && !left.contains('[') && !left.contains('(')
        }) && !raw.trim_start().starts_with("let ");
    if has(TokenKind::ColonEq) && binding_list {
        syntax(diagnostics);
    }
    if raw.contains(": (") && raw.contains(") :=") {
        push_diag(diagnostics, Code::MissingLet, lo, hi);
    }
    if has(TokenKind::DotDotEq) && raw.ends_with("..=") {
        syntax(diagnostics);
    }
    if has(TokenKind::Char)
        && (has(TokenKind::DotDot) || has(TokenKind::DotDotEq))
        && raw.contains("'..")
    {
        syntax(diagnostics);
    }
    if has(TokenKind::KwFn)
        && has(TokenKind::Dollar)
        && has(TokenKind::Lt)
        && raw.starts_with("fn run[$")
    {
        syntax(diagnostics);
    }
    if range_chained(kinds) {
        syntax(diagnostics);
    }
    if raw.contains(":=") && raw.trim_start().starts_with("mut ") {
        syntax(diagnostics);
    }
    if raw.contains(":=") && raw.matches(":=").count() > 1 && raw.contains(" if ") {
        syntax(diagnostics);
    }
    if raw.starts_with("return ") && raw.contains(":=") && raw.contains(" if ") {
        syntax(diagnostics);
    }
    if raw.contains(": a, b :=") {
        syntax(diagnostics);
    }
    if raw.contains("|>") && raw.ends_with(':') {
        syntax(diagnostics);
    }
    if raw.starts_with("if ")
        && raw.matches(':').count() > 1
        && (raw.ends_with(':') || raw.contains("): "))
        && !raw.contains("fn():")
    {
        syntax(diagnostics);
    }
    if raw.contains("callback: fn") && has(TokenKind::Dollar) && has(TokenKind::Comma) {
        syntax(diagnostics);
    }
    if has(TokenKind::FatArrow) && has(TokenKind::LBrace) && has(TokenKind::Eq) {
        syntax(diagnostics);
    }
    if has(TokenKind::FatArrow) && raw.contains("(mut ") {
        syntax(diagnostics);
    }
    if has(TokenKind::KwFor)
        && raw
            .split_once(" in ")
            .is_some_and(|(pattern, _)| pattern.contains(" mut ") || pattern.contains("{ mut "))
    {
        syntax(diagnostics);
    }
    if raw.contains(": type)") || raw.contains(": type,") {
        syntax(diagnostics);
    }
    if raw.contains('[')
        && !raw.contains("::[")
        && ((raw.contains("] {") && raw.chars().next().is_some_and(char::is_uppercase))
            || (raw.contains("]::") && raw.chars().next().is_some_and(char::is_uppercase)))
    {
        syntax(diagnostics);
    }
    if matches!(container, Some(TokenKind::KwTrait | TokenKind::KwImpl))
        && kinds.first() == Some(&TokenKind::KwFn)
        && parameter_without_type(raw)
    {
        syntax(diagnostics);
    }
    if matches!(
        container,
        Some(TokenKind::KwTrait | TokenKind::KwImpl | TokenKind::KwFn)
    ) && raw.starts_with("mut ")
        && raw.contains(':')
        && !raw.starts_with("mut self")
    {
        syntax(diagnostics);
    }
    if container == Some(TokenKind::KwData)
        && kinds.first() == Some(&TokenKind::Ident)
        && has(TokenKind::Eq)
        && !has(TokenKind::Colon)
    {
        syntax(diagnostics);
    }
    if in_enum
        && kinds.first() == Some(&TokenKind::Ident)
        && (raw.ends_with(':')
            || has(TokenKind::Eq) && has(TokenKind::LParen) && !has(TokenKind::Arrow))
    {
        syntax(diagnostics);
    }
    if container == Some(TokenKind::KwTests) && kinds.first() == Some(&TokenKind::KwTests) {
        syntax(diagnostics);
    }
    if container.is_some() && kinds.first() == Some(&TokenKind::KwTests) {
        syntax(diagnostics);
    }
    if raw.contains(": (") && raw.contains("..., ") {
        syntax(diagnostics);
    }
    if has(TokenKind::FatArrow)
        && kinds.iter().enumerate().any(|(index, kind)| {
            *kind == TokenKind::String
                && tokens
                    .text(TokenIdx::from_raw(as_u32(start + index)), source)
                    .starts_with("r\"")
        })
    {
        syntax(diagnostics);
    }
    if has(TokenKind::FatArrow)
        && kinds.iter().enumerate().any(|(index, kind)| {
            *kind == TokenKind::Number
                && tokens
                    .text(TokenIdx::from_raw(as_u32(start + index)), source)
                    .chars()
                    .last()
                    .is_some_and(char::is_alphabetic)
        })
    {
        syntax(diagnostics);
    }
    if raw.starts_with("data ") || raw.starts_with("enum ") {
        return;
    }
    if raw.contains(" = \"")
        && raw.contains(':')
        && !raw.contains(":=")
        && tokens.line_indent[tokens.line_of(lo)] > 0
    {
        push_diag(diagnostics, Code::MissingLet, lo, hi);
    }
}

fn between_first_pair(
    kinds: &[TokenKind],
    open: TokenKind,
    close: TokenKind,
) -> Option<std::ops::Range<usize>> {
    let start = kinds.iter().position(|kind| *kind == open)? + 1;
    let end = kinds[start..].iter().position(|kind| *kind == close)? + start;
    Some(start..end)
}

fn parameter_without_type(raw: &str) -> bool {
    let Some(open) = raw.find('(') else {
        return false;
    };
    let Some(close) = raw[open + 1..].find(')').map(|at| at + open + 1) else {
        return false;
    };
    raw[open + 1..close]
        .split(',')
        .map(str::trim)
        .any(|parameter| {
            !parameter.is_empty()
                && parameter != "self"
                && parameter != "mut self"
                && !parameter.contains(':')
        })
}

fn named_before_positional(kinds: &[TokenKind]) -> bool {
    let mut paren_depth = 0_usize;
    let mut nested_depth = 0_usize;
    let mut saw_named = [false; 32];
    let mut segment_named = [false; 32];
    for &kind in kinds {
        match kind {
            TokenKind::LParen => {
                paren_depth = (paren_depth + 1).min(31);
                saw_named[paren_depth] = false;
                segment_named[paren_depth] = false;
            }
            TokenKind::RParen => {
                let depth = paren_depth;
                if nested_depth == 0 && saw_named[depth] && !segment_named[depth] {
                    return true;
                }
                paren_depth = paren_depth.saturating_sub(1);
            }
            TokenKind::LBracket | TokenKind::LBrace if paren_depth > 0 => nested_depth += 1,
            TokenKind::RBracket | TokenKind::RBrace if paren_depth > 0 => nested_depth -= 1,
            TokenKind::Eq if paren_depth > 0 && nested_depth == 0 => {
                let depth = paren_depth;
                segment_named[depth] = true;
                saw_named[depth] = true;
            }
            TokenKind::Comma if paren_depth > 0 && nested_depth == 0 => {
                let depth = paren_depth;
                if saw_named[depth] && !segment_named[depth] {
                    return true;
                }
                segment_named[depth] = false;
            }
            _ => {}
        }
    }
    false
}

fn range_chained(kinds: &[TokenKind]) -> bool {
    let mut depth = 0_usize;
    let mut seen = [false; 32];
    for &kind in kinds {
        match kind {
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => {
                depth = (depth + 1).min(31);
                seen[depth] = false;
            }
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                seen[depth] = false;
                depth = depth.saturating_sub(1);
            }
            TokenKind::Comma | TokenKind::Colon | TokenKind::FatArrow => seen[depth] = false,
            TokenKind::DotDot | TokenKind::DotDotEq => {
                if seen[depth] {
                    return true;
                }
                seen[depth] = true;
            }
            _ => {}
        }
    }
    false
}

fn validate_comments(source: &str, tokens: &TokenBuf, diagnostics: &mut Vec<Diagnostic>) {
    let mut module_blocks = 0_u8;
    for (index, kind) in tokens.com_kind.iter().copied().enumerate() {
        if kind == CommentKind::ModuleDoc {
            let starts_block = index == 0
                || source.as_bytes()
                    [tokens.com_end[index - 1] as usize..tokens.com_start[index] as usize]
                    .windows(2)
                    .any(|pair| pair == b"\n\n");
            if starts_block {
                module_blocks += 1;
                if module_blocks > 1 {
                    push_diag(
                        diagnostics,
                        Code::DocCommentWithoutTarget,
                        tokens.com_start[index],
                        tokens.com_end[index],
                    );
                }
            }
            continue;
        }
        if kind != CommentKind::Doc {
            continue;
        }
        let end = tokens.com_end[index] as usize;
        if tokens.com_start.get(index + 1).is_some_and(|next| {
            *next as usize >= end && source[end..*next as usize].trim().is_empty()
        }) {
            continue;
        }
        let next_token = tokens
            .start
            .partition_point(|start| (*start as usize) < end);
        let Some(&next_start) = tokens.start.get(next_token) else {
            push_diag(
                diagnostics,
                Code::DocCommentWithoutTarget,
                tokens.com_start[index],
                tokens.com_end[index],
            );
            continue;
        };
        let gap = &source[end..next_start as usize];
        let blank_line = gap.as_bytes().windows(2).any(|pair| pair == b"\n\n")
            || gap.as_bytes().windows(4).any(|pair| pair == b"\r\n\r\n");
        let target = tokens.kind[next_token];
        let target_line = tokens.line_of(next_start);
        let target_indent = tokens.line_indent[target_line];
        let comment_line = tokens.line_of(tokens.com_start[index]);
        let comment_indent = u16::try_from(
            source[tokens.line_start[comment_line] as usize..tokens.com_start[index] as usize]
                .bytes()
                .take_while(|byte| *byte == b' ')
                .count(),
        )
        .unwrap_or(u16::MAX);
        let declaration = matches!(
            target,
            TokenKind::KwFn
                | TokenKind::KwData
                | TokenKind::KwEnum
                | TokenKind::KwTrait
                | TokenKind::KwImpl
                | TokenKind::KwType
                | TokenKind::KwPub
                | TokenKind::At
        ) || target == TokenKind::Ident
            && target_indent > 0
            && !tokens
                .kind
                .get(next_token + 1)
                .is_some_and(|kind| matches!(kind, TokenKind::ColonEq | TokenKind::Eq));
        if blank_line || target_indent != comment_indent || !declaration {
            push_diag(
                diagnostics,
                Code::DocCommentWithoutTarget,
                tokens.com_start[index],
                tokens.com_end[index],
            );
        }
    }
}

fn validate_continuations(_source: &str, tokens: &TokenBuf, diagnostics: &mut Vec<Diagnostic>) {
    let ends = tokens.line_token_ends();
    let mut delimiter_depth = 0_i32;
    let mut previous: Option<(usize, usize)> = None;
    let mut previous_was_decorator = false;
    let mut nested_suite: Option<(u16, bool)> = None;
    let mut bracket_statement_indent = None;
    for (line, &end) in ends.iter().enumerate() {
        let Some(first_token) = tokens.line_tok[line].get() else {
            continue;
        };
        let first = first_token.idx();
        let kinds = &tokens.kind[first..end];
        let lo = tokens.start[first];
        let hi = tokens.end[end - 1];
        if previous_was_decorator
            && kinds.first() == Some(&TokenKind::KwType)
            && kinds.contains(&TokenKind::Eq)
        {
            push_diag(diagnostics, Code::SyntaxError, lo, hi);
        }
        previous_was_decorator = kinds.first() == Some(&TokenKind::At);

        if delimiter_depth > 0
            && matches!(kinds.first(), Some(TokenKind::LParen | TokenKind::LBracket))
            && previous.is_some_and(|(_, previous_end)| {
                !matches!(
                    tokens.kind[previous_end - 1],
                    TokenKind::Comma | TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace
                )
            })
        {
            push_diag(diagnostics, Code::SyntaxError, lo, hi);
        }

        let depth_at_line_start = delimiter_depth;
        let mut local_depth = delimiter_depth;
        let mut nested_colon = false;
        for (index, &kind) in kinds.iter().enumerate() {
            if kind.is_open_delimiter() {
                local_depth += 1;
            } else if kind.is_close_delimiter() {
                local_depth -= 1;
            } else if kind == TokenKind::Colon && local_depth > 0 && index + 1 == kinds.len() {
                nested_colon = true;
            }
        }
        if nested_colon
            && let Some(next_line) = ((line + 1)..tokens.line_tok.len())
                .find(|candidate| tokens.line_tok[*candidate].get().is_some())
        {
            let next = tokens.line_tok[next_line].idx();
            if !kinds.iter().any(|kind| {
                matches!(
                    kind,
                    TokenKind::KwFn
                        | TokenKind::KwIf
                        | TokenKind::KwFor
                        | TokenKind::KwWhile
                        | TokenKind::KwMatch
                        | TokenKind::KwElse
                        | TokenKind::FatArrow
                )
            }) {
                push_diag(diagnostics, Code::TrailingBlockPosition, lo, hi);
            }
            let required_indent = bracket_statement_indent
                .map_or(tokens.line_indent[line], |base: u16| {
                    base.max(tokens.line_indent[line])
                });
            if tokens.line_indent[next_line] <= required_indent {
                push_diag(
                    diagnostics,
                    Code::UnexpectedIndentation,
                    tokens.start[next],
                    tokens.end[next],
                );
            } else {
                nested_suite = Some((
                    tokens.line_indent[next_line],
                    kinds.contains(&TokenKind::KwFn),
                ));
            }
        }
        let net_closes = local_depth < depth_at_line_start;
        if nested_suite.is_some_and(|(_, closure)| closure)
            && net_closes
            && !kinds.first().is_some_and(|kind| kind.is_close_delimiter())
            && kinds.first() != Some(&TokenKind::Comma)
        {
            push_diag(diagnostics, Code::SyntaxError, lo, hi);
        }

        if delimiter_depth == 0
            && matches!(
                kinds.first(),
                Some(
                    TokenKind::Plus
                        | TokenKind::Minus
                        | TokenKind::Star
                        | TokenKind::Slash
                        | TokenKind::Percent
                        | TokenKind::Amp
                        | TokenKind::Pipe
                        | TokenKind::Caret
                        | TokenKind::PipeGt
                )
            )
        {
            let range_pattern =
                kinds.first() == Some(&TokenKind::Minus) && kinds.contains(&TokenKind::FatArrow);
            let previous_allows_pipe = kinds.first() == Some(&TokenKind::PipeGt)
                && previous
                    .is_some_and(|(start, finish)| !same_line_suite(&tokens.kind[start..finish]));
            if !previous_allows_pipe && !range_pattern {
                push_diag(diagnostics, Code::SyntaxError, lo, hi);
            }
        }
        if delimiter_depth == 0
            && kinds.first() == Some(&TokenKind::Dot)
            && previous.is_some_and(|(start, finish)| {
                tokens.kind[start..finish].contains(&TokenKind::PipeGt)
                    || same_line_suite(&tokens.kind[start..finish])
            })
        {
            push_diag(diagnostics, Code::SyntaxError, lo, hi);
        }

        for &kind in kinds {
            if kind.is_open_delimiter() {
                delimiter_depth += 1;
            } else if kind.is_close_delimiter() {
                delimiter_depth -= 1;
            }
        }
        if delimiter_depth <= 0 {
            nested_suite = None;
            bracket_statement_indent = None;
        } else if depth_at_line_start == 0 {
            bracket_statement_indent = Some(tokens.line_indent[line]);
        }
        previous = Some((first, end));
    }
}

fn same_line_suite(kinds: &[TokenKind]) -> bool {
    let mut depth = 0_i32;
    kinds.iter().enumerate().any(|(index, kind)| {
        match kind {
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => depth -= 1,
            _ => {}
        }
        *kind == TokenKind::Colon && depth == 0 && index + 1 < kinds.len()
    })
}

fn old_bound_plus(kinds: &[TokenKind]) -> bool {
    let Some(less) = kinds.iter().position(|&kind| kind == TokenKind::Lt) else {
        return false;
    };
    kinds[less + 1..]
        .iter()
        .take_while(|&&kind| {
            !matches!(
                kind,
                TokenKind::Colon | TokenKind::RBracket | TokenKind::LParen
            )
        })
        .any(|&kind| kind == TokenKind::Plus)
}

fn old_row_comma(kinds: &[TokenKind]) -> bool {
    let Some(dollar) = kinds.iter().rposition(|&kind| kind == TokenKind::Dollar) else {
        return false;
    };
    let tail = &kinds[dollar + 1..];
    if tail.first() == Some(&TokenKind::Dot) {
        return false;
    }
    let Some(comma) = tail.iter().position(|&kind| kind == TokenKind::Comma) else {
        return false;
    };
    !tail[..comma]
        .iter()
        .any(|&kind| matches!(kind, TokenKind::RBracket | TokenKind::RParen))
}

fn validate_delimiters(tokens: &TokenBuf, diagnostics: &mut Vec<Diagnostic>) {
    let mut stack = Vec::<(TokenKind, usize)>::new();
    for (index, &kind) in tokens.kind.iter().enumerate() {
        if kind.is_open_delimiter() {
            stack.push((kind, index));
        } else if kind.is_close_delimiter() {
            let matches = stack.last().is_some_and(|&(open, _)| {
                matches!(
                    (open, kind),
                    (TokenKind::LParen, TokenKind::RParen)
                        | (TokenKind::LBracket, TokenKind::RBracket)
                        | (TokenKind::LBrace, TokenKind::RBrace)
                )
            });
            if matches {
                stack.pop();
            } else {
                push_diag(
                    diagnostics,
                    Code::UnmatchedDelimiter,
                    tokens.start[index],
                    tokens.end[index],
                );
            }
        }
    }
    for (_, index) in stack {
        push_diag(
            diagnostics,
            Code::UnclosedDelimiter,
            tokens.start[index],
            tokens.end[index],
        );
    }
}

fn validate_indentation(tokens: &TokenBuf, diagnostics: &mut Vec<Diagnostic>) {
    let mut active = vec![0_u16];
    let mut depth = 0_i32;
    let line_end = tokens.line_token_ends();
    for (line, &end) in line_end.iter().enumerate() {
        let Some(first) = tokens.line_tok[line].get() else {
            continue;
        };
        if depth == 0 {
            let indent = tokens.line_indent[line];
            if indent > *active.last().expect("indent stack") {
                active.push(indent);
            } else if indent < *active.last().expect("indent stack") {
                while active.last().is_some_and(|&value| value > indent) {
                    active.pop();
                }
                if active.last() != Some(&indent) {
                    push_diag(
                        diagnostics,
                        Code::InvalidDedent,
                        tokens.start[first.idx()],
                        tokens.end[first.idx()],
                    );
                    active.push(indent);
                }
            }
        }
        for &kind in &tokens.kind[first.idx()..end] {
            if kind.is_open_delimiter() {
                depth += 1;
            }
            if kind.is_close_delimiter() {
                depth = (depth - 1).max(0);
            }
        }
    }
}

fn push_diag(output: &mut Vec<Diagnostic>, code: Code, lo: u32, hi: u32) {
    output.push(Diagnostic::error(
        code,
        Span {
            file: FileId::from_raw(0),
            lo,
            hi,
        },
    ));
}

fn deduplicate(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by_key(|diagnostic| (diagnostic.primary.lo, diagnostic.code as u16));
    diagnostics.dedup_by_key(|diagnostic| (diagnostic.primary.lo, diagnostic.code as u16));
}

fn as_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX - 1)
}

#[cfg(test)]
mod tests {
    use crate::{SyntaxKind, parse};

    #[test]
    fn flat_tree_has_typed_function_and_dyn_type() {
        let source = "fn show(value: dyn Display) -> string:\n    value.to_string()\n";
        let parsed = parse(source.as_bytes());
        assert!(parsed.is_ok(), "{:?}", parsed.diagnostic_codes());
        let function = parsed.tree.root().children().next().expect("function");
        assert_eq!(function.kind(), SyntaxKind::FnDecl);
        assert!(
            function
                .descendants()
                .any(|node| node.kind() == SyntaxKind::DynType)
        );
        assert_eq!(parsed.tree.reconstruct(&parsed.tokens, source), source);
    }

    #[test]
    fn delimiter_recovery_reports_and_continues() {
        let parsed = parse(b"first := [1, 2)\nsecond := +2\n");
        assert!(
            parsed
                .diagnostic_codes()
                .iter()
                .any(|code| code.as_str() == "unmatched-delimiter")
        );
        assert!(parsed.tree.len() >= 3);
    }
}
