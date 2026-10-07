//! The full parser (syntax.md §4.4): recursive descent over the token
//! stream that the layout cursor (§4.2) interleaves with `NEWLINE`,
//! `INDENT` and `DEDENT`, emitting events that build the green tree. Each
//! diagnostic comes from the grammar point that rejects the input; a
//! statement reports its first error only, and recovery skips to the
//! statement's end.

mod exprs;
mod items;
mod patterns;
mod stmts;
mod types;

use std::collections::HashMap;

use hd_base::{FileId, ItemIdx, NodeIdx, Span, Symbol, TokenIdx};
use hd_diag::{Code, Diagnostic};
use hd_intern::Interner;

use crate::green::{Event, build};
use crate::layout::SuiteKind;
use crate::{
    CommentKind, GreenTree, Layout, LayoutCursor, SyntaxKind, TokenBuf, TokenKind, TokenOrLayout,
    lex,
};

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
    let mut parser = Parser::new(text, &lexed.tokens, &lexed.diagnostics);
    parser.source_file();
    let mut diagnostics = lexed.diagnostics.clone();
    diagnostics.extend(parser.finish_diagnostics());
    let tree = build(&mut parser.events, &parser.layouts, &[]);
    let (items, symbols) = build_item_index(&tree, &lexed.tokens, text);
    diagnostics.sort_by_key(|diagnostic| (diagnostic.primary.lo, diagnostic.code as u16));
    diagnostics.dedup_by_key(|diagnostic| (diagnostic.primary.lo, diagnostic.code as u16));
    Parse {
        tokens: lexed.tokens,
        tree,
        diagnostics,
        items,
        symbols,
    }
}

/// Where an expression stands, which decides the suite forms it may take
/// (02-grammar.md: `suite_statement`, `closed_expression`,
/// `inline_expression`, `continued_expression`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ctx {
    /// A statement's start: a binding chain may end in any suite.
    Stmt,
    /// A right-hand side: a suite expression, a trailing block call, or a
    /// closed expression.
    Rhs,
    /// A same-line suite body: no `if`, `match` or indented suite.
    Inline,
    /// Followed by another token outside brackets: no suite.
    Closed,
    /// Inside brackets: any suite.
    Expr,
    /// Inside brackets and followed by another token: no same-line suite at
    /// its end.
    Continued,
}

impl Ctx {
    fn inside_brackets(self) -> bool {
        matches!(self, Self::Expr | Self::Continued)
    }

    /// The context of a header expression (`if`, `while`, `for ... in`).
    fn header(self) -> Self {
        if self.inside_brackets() {
            Self::Continued
        } else {
            Self::Closed
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Marker(usize);

#[derive(Clone, Copy)]
pub(crate) struct Done {
    start: usize,
    kind: SyntaxKind,
}

impl Done {
    pub(crate) fn kind(self) -> SyntaxKind {
        self.kind
    }
}

const MAX_NESTING: u32 = 160;

pub(crate) struct Parser<'t> {
    src: &'t str,
    toks: &'t TokenBuf,
    cur: LayoutCursor<'t>,
    events: Vec<Event>,
    layouts: Vec<(TokenIdx, Layout)>,
    errors: Vec<(Code, u32, u32)>,
    /// Spans the lexer already reported: no second diagnostic there.
    lexed: Vec<(u32, u32)>,
    /// The first opener the lexer found unclosed.
    unclosed_from: u32,
    /// The current statement already reported its error.
    stmt_errored: bool,
    /// The last construct consumed ended in a suite.
    suite_closed: bool,
    /// The last suite consumed was a same-line suite.
    suite_inline: bool,
    nesting: u32,
    attached: Vec<bool>,
}

impl<'t> Parser<'t> {
    fn new(src: &'t str, toks: &'t TokenBuf, lexed: &[Diagnostic]) -> Self {
        let mut spans: Vec<(u32, u32)> = lexed
            .iter()
            .map(|diagnostic| (diagnostic.primary.lo, diagnostic.primary.hi))
            .collect();
        spans.sort_unstable();
        Self {
            src,
            toks,
            cur: LayoutCursor::new(toks),
            events: Vec::with_capacity(toks.len() * 3),
            layouts: Vec::new(),
            errors: Vec::new(),
            lexed: spans,
            unclosed_from: lexed
                .iter()
                .filter(|diagnostic| diagnostic.code == Code::UnclosedDelimiter)
                .map(|diagnostic| diagnostic.primary.lo)
                .min()
                .unwrap_or(u32::MAX),
            stmt_errored: false,
            suite_closed: false,
            suite_inline: false,
            nesting: 0,
            attached: vec![false; toks.com_kind.len()],
        }
    }

    fn finish_diagnostics(&mut self) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for &(code, at) in &self.cur.errors {
            let (lo, hi) = self.token_span(at);
            if !self.lexed_covers(lo) {
                out.push(diag(code, lo, hi));
            }
        }
        for &(code, lo, hi) in &self.errors {
            out.push(diag(code, lo, hi));
        }
        self.doc_comments(&mut out);
        out
    }

    // ------------------------------------------------------------ tokens

    fn pos(&self) -> usize {
        self.cur.pos()
    }

    /// The next real token's kind, or `None` at a layout token or the end.
    pub(crate) fn current(&mut self) -> Option<TokenKind> {
        match self.cur.peek() {
            TokenOrLayout::Token(token) => Some(self.toks.kind(token)),
            _ => None,
        }
    }

    pub(crate) fn layout(&mut self) -> Option<Layout> {
        match self.cur.peek() {
            TokenOrLayout::Layout { kind, .. } => Some(kind),
            _ => None,
        }
    }

    pub(crate) fn at_eof(&mut self) -> bool {
        self.cur.peek() == TokenOrLayout::Eof
    }

    pub(crate) fn at(&mut self, kind: TokenKind) -> bool {
        self.current() == Some(kind)
    }

    pub(crate) fn at_layout(&mut self, kind: Layout) -> bool {
        self.layout() == Some(kind)
    }

    /// The raw token `n` places after the current one (layout ignored).
    pub(crate) fn nth(&mut self, n: usize) -> Option<TokenKind> {
        self.current()?;
        self.toks.kind.get(self.pos() + n).copied()
    }

    pub(crate) fn nth_text(&mut self, n: usize) -> &'t str {
        let index = self.pos() + n;
        if index >= self.toks.len() {
            return "";
        }
        self.toks.text(TokenIdx::from_raw(u32_of(index)), self.src)
    }

    pub(crate) fn at_word(&mut self, word: &str) -> bool {
        self.at(TokenKind::Ident) && self.nth_text(0) == word
    }

    /// The token `n` places ahead begins its physical line.
    pub(crate) fn nth_line_first(&self, n: usize) -> bool {
        let index = self.pos() + n;
        index < self.toks.len() && self.toks.is_line_first(TokenIdx::from_raw(u32_of(index)))
    }

    /// The token `n` places ahead touches the one before it.
    pub(crate) fn nth_glued(&self, n: usize) -> bool {
        let index = self.pos() + n;
        index > 0 && index < self.toks.len() && self.toks.end[index - 1] == self.toks.start[index]
    }

    pub(crate) fn bump(&mut self) {
        match self.cur.peek() {
            TokenOrLayout::Token(token) => {
                self.events.push(Event::Token(token));
                self.suite_closed = false;
                self.cur.bump();
            }
            TokenOrLayout::Layout { kind, at } => {
                self.layouts.push((at, kind));
                self.cur.bump();
            }
            TokenOrLayout::Eof => {}
        }
    }

    pub(crate) fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(crate) fn eat_layout(&mut self, kind: Layout) -> bool {
        if self.at_layout(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, kind: TokenKind) -> bool {
        if self.eat(kind) {
            return true;
        }
        self.error(Code::SyntaxError);
        false
    }

    pub(crate) fn eat_name(&mut self) -> bool {
        if matches!(self.current(), Some(TokenKind::Ident | TokenKind::RawIdent)) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect_name(&mut self) -> bool {
        if self.eat_name() {
            return true;
        }
        self.error(Code::SyntaxError);
        false
    }

    // ----------------------------------------------------------- markers

    pub(crate) fn start(&mut self) -> Marker {
        let at = self.events.len();
        self.events.push(Event::Start {
            kind: SyntaxKind::Error,
            forward_parent: 0,
            open: false,
        });
        Marker(at)
    }

    pub(crate) fn complete(&mut self, marker: Marker, kind: SyntaxKind) -> Done {
        if let Event::Start {
            kind: slot, open, ..
        } = &mut self.events[marker.0]
        {
            *slot = kind;
            *open = true;
        }
        self.events.push(Event::Finish);
        Done {
            start: marker.0,
            kind,
        }
    }

    /// Drops a marker that wraps nothing (its tokens stay with the parent).
    pub(crate) fn abandon(&mut self, marker: Marker) {
        if marker.0 + 1 == self.events.len() {
            self.events.pop();
        }
    }

    pub(crate) fn precede(&mut self, done: Done) -> Marker {
        let marker = self.start();
        if let Event::Start { forward_parent, .. } = &mut self.events[done.start] {
            *forward_parent = u32_of(marker.0 - done.start);
        }
        marker
    }

    /// One token wrapped in a node.
    pub(crate) fn token_node(&mut self, kind: SyntaxKind) -> Done {
        let marker = self.start();
        self.bump();
        self.complete(marker, kind)
    }

    // ------------------------------------------------------------ errors

    fn token_span(&self, at: usize) -> (u32, u32) {
        if at < self.toks.len() {
            (self.toks.start[at], self.toks.end[at])
        } else {
            let end = u32_of(self.src.len());
            (end, end)
        }
    }

    fn lexed_covers(&self, offset: u32) -> bool {
        // Past an unclosed delimiter every later line is inside it.
        if offset > self.unclosed_from {
            return true;
        }
        let index = self.lexed.partition_point(|&(lo, _)| lo <= offset);
        self.lexed[..index]
            .iter()
            .any(|&(_, hi)| offset < hi.max(1))
            || self.lexed[..index].iter().any(|&(lo, _)| lo == offset)
    }

    /// Reports `code` at the current token, once per statement, and never
    /// where the lexer already reported.
    pub(crate) fn error(&mut self, code: Code) {
        let at = self.pos();
        self.error_at(code, at);
    }

    pub(crate) fn error_at(&mut self, code: Code, at: usize) {
        if self.stmt_errored {
            return;
        }
        self.stmt_errored = true;
        if at < self.toks.len()
            && matches!(self.toks.kind[at], TokenKind::Error | TokenKind::Semicolon)
        {
            return;
        }
        if at >= self.toks.len() && !self.lexed.is_empty() {
            return;
        }
        let (lo, hi) = self.token_span(at);
        if self.lexed_covers(lo) {
            return;
        }
        self.errors.push((code, lo, hi));
    }

    /// A diagnostic that does not end the statement's reporting (a rule
    /// with its own code whose construct still parses).
    pub(crate) fn report(&mut self, code: Code, at: usize) {
        let (lo, hi) = self.token_span(at);
        if !self.lexed_covers(lo) {
            self.errors.push((code, lo, hi));
        }
    }

    /// Skips to the end of the statement: the next `NEWLINE` at this
    /// level, or before a `DEDENT`, wrapping the junk in an `Error` node.
    pub(crate) fn recover_line(&mut self) {
        let marker = self.start();
        let mut indents = 0_u32;
        loop {
            match self.layout() {
                Some(Layout::Newline) if indents == 0 => break,
                Some(Layout::Dedent) if indents == 0 => break,
                Some(Layout::Indent) => indents += 1,
                Some(Layout::Dedent) => indents -= 1,
                _ => {}
            }
            if self.at_eof() {
                break;
            }
            self.bump();
        }
        self.complete(marker, SyntaxKind::Error);
    }

    /// Skips one token into an `Error` node.
    pub(crate) fn junk(&mut self) {
        let marker = self.start();
        self.bump();
        self.complete(marker, SyntaxKind::Error);
    }

    // ------------------------------------------------------------ suites

    /// After a suite-introducing `:`: the suite body as a `Block`.
    pub(crate) fn suite(&mut self, closure: bool, inline_ctx: Ctx) -> Done {
        let marker = self.start();
        if !self.enter() {
            self.skip_balanced();
            return self.complete(marker, SyntaxKind::Block);
        }
        let kind = self.cur.open_suite(closure);
        self.leave();
        self.nesting += 1;
        match kind {
            SuiteKind::SameLine => {
                self.inline_statement(inline_ctx);
                self.cur.close_inline();
                let at = TokenIdx::from_raw(u32_of(self.pos()));
                self.layouts.push((at, Layout::SuiteEnd));
                self.suite_inline = true;
            }
            SuiteKind::Indented => {
                self.indented_body();
                self.suite_inline = false;
            }
            SuiteKind::Missing => {
                self.error(Code::SyntaxError);
                self.suite_inline = false;
            }
        }
        self.nesting -= 1;
        self.suite_closed = true;
        self.complete(marker, SyntaxKind::Block)
    }

    /// `NEWLINE INDENT statement+ DEDENT`, the cursor having queued the first two.
    pub(crate) fn indented_body(&mut self) {
        self.eat_layout(Layout::Newline);
        self.eat_layout(Layout::Indent);
        self.statements();
        self.eat_layout(Layout::Dedent);
    }

    /// Statements until the block's `DEDENT`.
    pub(crate) fn statements(&mut self) {
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
            self.statement();
            self.statement_end();
            if self.pos() == before && self.layout().is_none() && !self.at_eof() {
                self.junk();
            }
        }
    }

    /// After a statement: its `NEWLINE`, unless it ended in a suite.
    pub(crate) fn statement_end(&mut self) {
        if self.eat_layout(Layout::Newline) {
            return;
        }
        if self.suite_closed && !self.suite_inline
            || self.at_layout(Layout::Dedent)
            || self.at_layout(Layout::Indent)
            || self.at_eof()
        {
            return;
        }
        self.error(Code::SyntaxError);
        self.recover_line();
        self.eat_layout(Layout::Newline);
    }

    /// An `INDENT` no suite opened: one error, then its lines are read as
    /// part of the current suite.
    pub(crate) fn stray_indent(&mut self) {
        self.stmt_errored = false;
        self.error(Code::SyntaxError);
        let marker = self.start();
        self.bump();
        self.statements();
        self.eat_layout(Layout::Dedent);
        self.complete(marker, SyntaxKind::Error);
    }

    // ----------------------------------------------------------- nesting

    pub(crate) fn enter(&mut self) -> bool {
        if self.nesting >= MAX_NESTING {
            let at = self.pos();
            if !self.errors.iter().any(|e| e.0 == Code::NestingTooDeep) {
                self.report(Code::NestingTooDeep, at);
            }
            self.stmt_errored = true;
            return false;
        }
        self.nesting += 1;
        true
    }

    pub(crate) fn leave(&mut self) {
        self.nesting -= 1;
    }

    /// Past the nesting limit: skips a balanced run of tokens.
    pub(crate) fn skip_balanced(&mut self) {
        let marker = self.start();
        let mut depth = 0_u32;
        while let Some(kind) = self.current() {
            if kind.opens() {
                depth += 1;
            } else if kind.closes() || kind == TokenKind::StrMid {
                if depth == 0 {
                    break;
                }
                if kind != TokenKind::StrMid {
                    depth -= 1;
                }
            } else if depth == 0 && kind == TokenKind::Comma {
                break;
            }
            self.bump();
        }
        self.complete(marker, SyntaxKind::Error);
    }

    // ------------------------------------------------------ doc comments

    /// `lex.doc.attach`: documentation lines directly above the token that
    /// starts a declaration or member, at its indentation.
    pub(crate) fn attach_docs(&mut self) {
        let at = self.pos();
        if at >= self.toks.len() {
            return;
        }
        let start = self.toks.start[at];
        let column = self.column(at);
        let mut index = self.toks.com_end.partition_point(|&end| end <= start);
        let mut next = start;
        while index > 0 {
            let comment = index - 1;
            if self.toks.com_kind[comment] != CommentKind::Doc {
                break;
            }
            let lo = self.toks.com_start[comment] as usize;
            let hi = self.toks.com_end[comment] as usize;
            let gap = &self.src.as_bytes()[hi..next as usize];
            let lines = gap.split(|byte| *byte == b'\n').count();
            if lines != 2 || !gap.iter().all(u8::is_ascii_whitespace) {
                break;
            }
            let line = self.toks.line_of(u32_of(lo));
            let own = self.toks.line_start[line] as usize
                + usize::from(self.toks.line_indent[line])
                == lo;
            if !own || self.toks.line_indent[line] != column {
                break;
            }
            self.attached[comment] = true;
            next = u32_of(lo);
            index -= 1;
        }
    }

    fn column(&self, at: usize) -> u16 {
        let line = self.toks.line_of(self.toks.start[at]);
        let line_start = self.toks.line_start[line];
        let before = &self.src.as_bytes()[line_start as usize..self.toks.start[at] as usize];
        if before.iter().all(|byte| *byte == b' ') {
            self.toks.line_indent[line]
        } else {
            u16::MAX
        }
    }

    /// `lex.doc.unattached`: an own-line documentation comment that no
    /// declaration claimed.
    fn doc_comments(&self, out: &mut Vec<Diagnostic>) {
        for index in 0..self.toks.com_kind.len() {
            if self.toks.com_kind[index] != CommentKind::Doc || self.attached[index] {
                continue;
            }
            let lo = self.toks.com_start[index];
            let line = self.toks.line_of(lo);
            let own = self.toks.line_start[line] + u32::from(self.toks.line_indent[line]) == lo;
            if own && self.toks.line_flags[line].0 & crate::LineFlags::CONTINUES_STRING == 0 {
                out.push(diag(
                    Code::DocCommentWithoutTarget,
                    lo,
                    self.toks.com_end[index],
                ));
            }
        }
    }
}

fn diag(code: Code, lo: u32, hi: u32) -> Diagnostic {
    Diagnostic::error(
        code,
        Span {
            file: FileId::from_raw(0),
            lo,
            hi,
        },
    )
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
            SyntaxKind::UseDecl | SyntaxKind::Error => continue,
            _ => ItemKind::Statement,
        };
        let name = crate::green::Decl::cast(child)
            .and_then(|decl| decl.name(tokens))
            .map_or(Symbol::NONE, |token| {
                symbols.intern(tokens.text(token, source).trim_matches('`'))
            });
        let item = ItemIdx::from_raw(u32_of(result.at.len()));
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

pub(crate) fn u32_of(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX - 1)
}

#[cfg(test)]
mod tests;
