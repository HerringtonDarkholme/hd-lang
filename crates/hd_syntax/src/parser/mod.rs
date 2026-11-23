//! The full parser (`crates/hd_syntax/DESIGN.md` §4.4): recursive descent over the token
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

use hd_base::HashMap;
use hd_base::SliceSort as _;
use hd_base::lines::LineIndex;

use hd_base::{FileId, ItemIdx, NodeIdx, Span, Symbol, TokenIdx};
use hd_diag::{Code, Diagnostic, DiagnosticFix, FixSafety};
use hd_intern::Interner;

use crate::green::{Event, build};
use crate::layout::SuiteKind;
use crate::lexer::NESTING_TOO_DEEP;
use crate::{
    CommentKind, GreenTree, Layout, LayoutCursor, SyntaxKind, TokenBuf, TokenKind, TokenOrLayout,
    lex,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ItemKind {
    Function,
    Struct,
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
    /// The top-level children that hold a parse error, in tree order
    /// ([`broken_items`]). Empty, with no work done, for a clean
    /// parse.
    pub broken: Vec<NodeIdx>,
    /// The parsed text's line index, built on the first
    /// [`Self::line_column`], so every position located in the file,
    /// from any thread, shares one pass over its bytes.
    lines: std::sync::OnceLock<LineIndex>,
    /// Where a `use` that imports a missing name goes, found on the
    /// first [`Self::use_insertion`].
    use_at: std::sync::OnceLock<UseInsertion>,
}

/// Where a `use` that imports a missing name goes ([`Parse::use_insertion`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UseInsertion {
    /// The byte the `use` line is inserted at.
    pub at: u32,
    /// It follows another `use`: the insertion starts a line at the end
    /// of that one. Otherwise it is a line of its own and a blank line.
    pub after_use: bool,
    /// The line ending the file uses there, `"\r\n"` or `"\n"`, so the
    /// added line does not mix endings.
    pub newline: &'static str,
}

impl Parse {
    /// The 1-based line and column, in characters, of byte `at` of
    /// `text`, the text this was parsed from (at most its length).
    #[must_use]
    pub fn line_column(&self, text: &str, at: usize) -> (usize, usize) {
        let lines = self.lines.get_or_init(|| LineIndex::new(text.as_bytes()));
        lines.line_column(at.min(text.len()))
    }

    /// Where a `use` declaration that imports a missing name goes in
    /// `text`, the text this was parsed from, and whether it follows
    /// another `use`. After one, it goes at the end of the line of the
    /// file's last top-level `use`. Otherwise it goes at the start of the
    /// file, or after the module documentation block and the blank line
    /// that follows it (`lex.doc.module`), which a `use` before it would
    /// detach. The line takes the file's line ending: the one of the line
    /// it follows, else the file's first. Found on the first call, by one
    /// pass over the top-level items; every later call shares it.
    #[must_use]
    pub fn use_insertion(&self, text: &str) -> UseInsertion {
        *self.use_at.get_or_init(|| {
            let to_u32 = |at: usize| u32::try_from(at).unwrap_or(u32::MAX);
            let after_line = |at: usize| {
                text.get(at..)
                    .and_then(|t| t.find('\n'))
                    .map_or(text.len(), |i| at + i)
            };
            // Whether the line ending at the `\n` at `end` is `\r\n`.
            let crlf_at = |end: usize| end > 0 && text.as_bytes().get(end - 1) == Some(&b'\r');
            let first = text.find('\n');
            let newline = |crlf: bool| if crlf { "\r\n" } else { "\n" };
            let file_newline = newline(first.is_some_and(crlf_at));
            let last_use = self
                .tree
                .root()
                .children()
                .filter(|c| c.kind() == SyntaxKind::UseDecl)
                .last();
            if let Some(u) = last_use {
                let (_, last) = self.tree.span_tokens(u.index());
                let end = after_line(self.tokens.end[last.idx()] as usize);
                // A `\r\n` line ends before its `\r`.
                if end < text.len() && crlf_at(end) {
                    return UseInsertion {
                        at: to_u32(end - 1),
                        after_use: true,
                        newline: "\r\n",
                    };
                }
                let newline = if end < text.len() { "\n" } else { file_newline };
                return UseInsertion {
                    at: to_u32(end),
                    after_use: true,
                    newline,
                };
            }
            let top = UseInsertion {
                at: 0,
                after_use: false,
                newline: file_newline,
            };
            // The module documentation comes before the first token.
            let first_token = self.tokens.start.first().map_or(u32::MAX, |&s| s);
            let before = self.tokens.com_start.partition_point(|&s| s < first_token);
            let Some(doc) = self.tokens.com_kind[..before]
                .iter()
                .rposition(|k| *k == CommentKind::ModuleDoc)
            else {
                return top;
            };
            // Past the end of its last line, and the blank line after it.
            let blank = after_line(self.tokens.com_end[doc] as usize);
            let at = after_line((blank + 1).min(text.len()));
            UseInsertion {
                at: to_u32((at + 1).min(text.len())),
                ..top
            }
        })
    }

    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Whether top-level child `node` holds a diagnostic of this parse,
    /// so no check reads it (`grammar.recovery.item`).
    #[must_use]
    pub fn is_broken(&self, node: NodeIdx) -> bool {
        !self.broken.is_empty()
            && self
                .broken
                .binary_search_by_key(&node.raw(), |n| n.raw())
                .is_ok()
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
    let (parsed, mut placed) = parser.finish_diagnostics();
    diagnostics.extend(parsed);
    placed.extend(lexed.diagnostics.iter().map(|d| d.primary.lo));
    placed.sort_stable();
    let tree = build(&mut parser.events);
    let (items, symbols) = build_item_index(&tree, &lexed.tokens, text);
    diagnostics.sort_stable_by_key(|diagnostic| (diagnostic.primary.lo, diagnostic.code as u16));
    diagnostics.dedup_by_key(|diagnostic| (diagnostic.primary.lo, diagnostic.code as u16));
    let broken = broken_items(&tree, &lexed.tokens, &placed, &parser.errored_items);
    Parse {
        tokens: lexed.tokens,
        tree,
        diagnostics,
        items,
        symbols,
        broken,
        lines: std::sync::OnceLock::new(),
        use_at: std::sync::OnceLock::new(),
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

/// Binary operators, prefix operators and postfix operations on one path
/// from an expression down to an operand, brackets included
/// (`grammar.limit.chain`). Each
/// link is a level of the syntax tree that every later walk takes by
/// recursion, and the nesting limit does not bound them: a chain loops in
/// the parser. Below it, check, fmt and build fit a pool worker's stack
/// (`hd_sched::pool::WORKER_STACK`), which `hd` gives its command thread.
#[cfg(not(target_family = "wasm"))]
pub(crate) const MAX_CHAIN_LINKS: u32 = 10_000;

/// The browser build's chain limit. A wasm frame lives on the engine's
/// own stack, which a page cannot raise (about 1 MiB in V8), so the
/// browser build takes shorter chains. Measured on Node 25's V8 with
/// the 2 MiB linear-memory stack `scripts/build-web-compiler.ts` links:
/// the first shape to fail, `f()()...` under 79 nested blocks, fails
/// between 1,200 and 1,300 links; `x |> f(_)...` past 1,500 and every
/// other shape past 1,900. 500 leaves more than twice that margin for
/// other engines.
#[cfg(target_family = "wasm")]
pub(crate) const MAX_CHAIN_LINKS: u32 = 500;

/// `grammar.limit.chain.error`.
const CHAIN_TOO_LONG: &str =
    "an expression chains more operators and postfix operations than the implementation limit";

/// `grammar.type.rest.elsewhere`.
const REST_ELSEWHERE: &str =
    "a rest element is only allowed last in a tuple type or a function parameter list";

/// `grammar.pattern.mut-let-only`.
const MUT_LET_ONLY: &str = "'mut' before a binding name is only valid in a 'let' pattern";

/// `grammar.impl.pub-method`.
const TRAIT_METHOD_VISIBILITY: &str =
    "trait methods inherit the trait's visibility and cannot be declared pub";

pub(crate) struct Parser<'t> {
    src: &'t str,
    toks: &'t TokenBuf,
    cur: LayoutCursor<'t>,
    events: Vec<Event>,
    errors: Vec<Diagnostic>,
    /// Spans the lexer already reported: no second diagnostic there.
    lexed: Vec<(u32, u32)>,
    /// The scope of each delimiter the lexer found unclosed: from the
    /// opener through the token that ended it, or to the end of the file.
    unclosed: Vec<(u32, u32)>,
    /// The current statement already reported its error.
    stmt_errored: bool,
    /// A top-level item is being parsed: an error raised now is its own,
    /// wherever it stands.
    in_item: bool,
    /// The top-level item being parsed raised an error, which may stand
    /// at a token past its end: a name missing before the next item's
    /// keyword is reported at that keyword.
    item_errored: bool,
    /// The first token of each top-level item that raised an error.
    errored_items: Vec<u32>,
    /// Alongside `errors`: whether a top-level item raised it.
    raised_in_item: Vec<bool>,
    /// The last construct consumed ended in a suite.
    suite_closed: bool,
    /// The last suite consumed was a same-line suite.
    suite_inline: bool,
    nesting: u32,
    /// The most links (`MAX_CHAIN_LINKS`) on any path of an expression
    /// finished since `binary` or `postfix` last took it: each takes it
    /// before its first operand, reads its operands' counts from it, and
    /// leaves the greater of the old value and its own count.
    links: u32,
    /// Opaque type occurrences are syntax only in a named function's
    /// return annotation, including its nested type arguments.
    opaque_result: bool,
    /// An opaque bound cannot itself declare another opaque occurrence.
    opaque_bound: bool,
    /// A named function's parameter type that starts with `fn` may end
    /// its result with a marker bound (`grammar.fn.crossing-parameter`).
    crossing_bound: bool,
    /// `_` stands for an inferred type inside a `let` annotation and an
    /// expression's type-argument list (`types.generic.placeholder.local`).
    infer_types: bool,
    attached: Vec<bool>,
    /// Reserved words the current top-level item tried to bind with `:=`:
    /// after that one error, the word reads as the name it was meant as.
    bound_keywords: Vec<TokenKind>,
    /// The type parameter names the last generic parameter list declared,
    /// and those of the trait or implementation whose members are being
    /// parsed: a method's `where` clause names only the latter
    /// (`trait.where.subject`).
    declared_generics: Vec<String>,
    enclosing_generics: Vec<String>,
}

impl<'t> Parser<'t> {
    fn new(src: &'t str, toks: &'t TokenBuf, lexed: &[Diagnostic]) -> Self {
        let mut spans: Vec<(u32, u32)> = lexed
            .iter()
            .map(|diagnostic| (diagnostic.primary.lo, diagnostic.primary.hi))
            .collect();
        spans.extend(
            lexed
                .iter()
                .filter(|diagnostic| diagnostic.code == Code::TabWhitespace)
                .filter_map(|diagnostic| tabbed_indentation(src, diagnostic.primary.lo)),
        );
        spans.sort_stable();
        Self {
            src,
            toks,
            cur: LayoutCursor::new(toks, src),
            events: Vec::with_capacity(toks.len() * 3),
            errors: Vec::new(),
            lexed: spans,
            unclosed: unclosed_scopes(toks, lexed),
            stmt_errored: false,
            in_item: false,
            item_errored: false,
            errored_items: Vec::new(),
            raised_in_item: Vec::new(),
            suite_closed: false,
            suite_inline: false,
            nesting: 0,
            links: 0,
            opaque_result: false,
            opaque_bound: false,
            crossing_bound: false,
            infer_types: false,
            attached: vec![false; toks.com_kind.len()],
            bound_keywords: Vec::new(),
            declared_generics: Vec::new(),
            enclosing_generics: Vec::new(),
        }
    }

    /// The parse's diagnostics, and the starts of those no top-level item
    /// raised, which belong to the item whose source holds them
    /// ([`broken_items`]).
    fn finish_diagnostics(&mut self) -> (Vec<Diagnostic>, Vec<u32>) {
        let mut out = Vec::new();
        for (code, at, message) in &self.cur.errors {
            let (lo, hi) = self.token_span(*at);
            if !self.lexed_covers(lo) {
                out.push(diag(*code, lo, hi, message.clone()));
            }
        }
        let mut placed: Vec<u32> = out.iter().map(|d| d.primary.lo).collect();
        for (d, &raised) in self.errors.iter().zip(&self.raised_in_item) {
            if !raised {
                placed.push(d.primary.lo);
            }
        }
        out.extend(self.errors.iter().cloned());
        let docs = out.len();
        self.doc_comments(&mut out);
        placed.extend(out[docs..].iter().map(|d| d.primary.lo));
        (out, placed)
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
                // A token the lexer rejected is the statement's first error.
                if !self.lexed.is_empty()
                    && self.lexed_covers(self.toks.start[token.raw() as usize])
                {
                    self.stmt_errored = true;
                }
                self.events.push(Event::Token(token));
                self.suite_closed = false;
                self.cur.bump();
            }
            TokenOrLayout::Layout { .. } => self.cur.bump(),
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

    /// Eats `kind`, or reports "expected `what`, found ...". `what` is the
    /// token as written, in quotes.
    pub(crate) fn expect_token(&mut self, kind: TokenKind, what: &str) -> bool {
        if self.eat(kind) {
            return true;
        }
        self.expected(what);
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

    /// Eats a name, or reports "expected `what`, found ...".
    pub(crate) fn expect_name(&mut self, what: &str) -> bool {
        if self.eat_name() {
            return true;
        }
        self.expected(what);
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
        // grammar.recovery.unclosed: the lines after an unclosed opener are
        // inside it until the item start that ended it. That token is
        // included, for the closers the parser then finds missing.
        if self
            .unclosed
            .iter()
            .any(|&(lo, end)| lo < offset && offset <= end)
        {
            return true;
        }
        let index = self.lexed.partition_point(|&(lo, _)| lo <= offset);
        self.lexed[..index]
            .iter()
            .any(|&(_, hi)| offset < hi.max(1))
            || self.lexed[..index].iter().any(|&(lo, _)| lo == offset)
    }

    /// A lexer error's span or an unclosed delimiter's scope reaches the
    /// end of the file, so an error found there is its fallout.
    fn lexed_reaches_end(&self) -> bool {
        let end = u32_of(self.src.len());
        self.unclosed.iter().any(|&(_, scope)| scope == u32::MAX)
            || self.lexed.iter().any(|&(_, hi)| hi >= end)
    }

    /// Reports `code` at the current token, once per statement, and never
    /// where the lexer already reported.
    pub(crate) fn error(&mut self, code: Code, message: &str) {
        let at = self.pos();
        self.error_at(code, at, message);
    }

    pub(crate) fn error_at(&mut self, code: Code, at: usize, message: &str) {
        self.raise(code, at, |_| message.to_owned());
    }

    /// A `syntax-error` at the current token: "expected `what`, found X".
    pub(crate) fn expected(&mut self, what: &str) {
        let at = self.pos();
        self.expected_at(what, at);
    }

    /// A `syntax-error` at token `at`: "expected `what`, found X".
    pub(crate) fn expected_at(&mut self, what: &str, at: usize) {
        self.raise(Code::SyntaxError, at, |parser| {
            format!("expected {what}, found {}", parser.found(at))
        });
    }

    fn raise(&mut self, code: Code, at: usize, message: impl FnOnce(&Self) -> String) {
        self.item_errored = true;
        if self.stmt_errored {
            return;
        }
        self.stmt_errored = true;
        if at < self.toks.len()
            && matches!(self.toks.kind[at], TokenKind::Error | TokenKind::Semicolon)
        {
            return;
        }
        if at >= self.toks.len() && self.lexed_reaches_end() {
            return;
        }
        let (lo, hi) = self.token_span(at);
        if self.lexed_covers(lo) {
            return;
        }
        self.cur.peek();
        let message = message(self);
        self.push_error(diag(code, lo, hi, message));
    }

    fn push_error(&mut self, diagnostic: Diagnostic) {
        self.errors.push(diagnostic);
        self.raised_in_item.push(self.in_item);
    }

    /// A diagnostic that does not end the statement's reporting (a rule
    /// with its own code whose construct still parses).
    pub(crate) fn report(&mut self, code: Code, at: usize, message: &str) {
        self.item_errored = true;
        let (lo, hi) = self.token_span(at);
        if !self.lexed_covers(lo) {
            self.push_error(diag(code, lo, hi, message));
        }
    }

    /// Attaches to the just-reported error only; suppressed errors have
    /// no row at `index`, so never borrow another statement's fix.
    pub(crate) fn fix_error(&mut self, index: usize, message: &str, edits: &[(u32, u32, &str)]) {
        self.error_fix(index, message, FixSafety::Exact, edits);
    }

    pub(crate) fn suggest_error(
        &mut self,
        index: usize,
        message: &str,
        edits: &[(u32, u32, &str)],
    ) {
        self.error_fix(index, message, FixSafety::Suggestion, edits);
    }

    fn error_fix(
        &mut self,
        index: usize,
        message: &str,
        safety: FixSafety,
        edits: &[(u32, u32, &str)],
    ) {
        let Some(diagnostic) = self.errors.get_mut(index) else {
            return;
        };
        let file = diagnostic.primary.file;
        diagnostic.fixes.push(DiagnosticFix {
            message: message.to_owned(),
            safety,
            edits: edits
                .iter()
                .map(|(lo, hi, text)| {
                    (
                        Span {
                            file,
                            lo: *lo,
                            hi: *hi,
                        },
                        (*text).to_owned(),
                    )
                })
                .collect(),
        });
    }

    /// What stands at token `at`, for an "expected ..., found ..." message:
    /// the layout the cursor would produce there, else the token.
    fn found(&self, at: usize) -> String {
        if at == self.cur.pos() {
            match self.cur.queued() {
                Some(Layout::Newline) => return "a line ending".to_owned(),
                Some(Layout::Indent) => return "an indented block".to_owned(),
                Some(Layout::Dedent) => return "the end of an indented block".to_owned(),
                None => {}
            }
        }
        self.toks.describe(at, self.src)
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
                Some(Layout::Newline) | None => {}
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
                self.suite_inline = true;
            }
            SuiteKind::Indented => {
                self.indented_body();
                self.suite_inline = false;
            }
            SuiteKind::Missing => {
                self.error(
                    Code::SyntaxError,
                    "expected an indented suite, found a line that is not indented deeper",
                );
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
        self.statements(false);
        self.eat_layout(Layout::Dedent);
    }

    /// Statements until the block's `DEDENT`. With `first_reported`, the
    /// first one already carries an error.
    pub(crate) fn statements(&mut self, mut first_reported: bool) {
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
                None => {}
            }
            if self.at_eof() {
                break;
            }
            let before = self.pos();
            self.stmt_errored = first_reported;
            first_reported = false;
            self.statement();
            self.statement_end();
            if self.pos() == before && self.layout().is_none() && !self.at_eof() {
                self.junk();
            }
        }
    }

    /// After a statement: its `NEWLINE`, unless it ended in a suite. A
    /// statement that reported an error also takes the lines its error
    /// left unread (`grammar.recovery.statement`).
    pub(crate) fn statement_end(&mut self) {
        let ended = self.eat_layout(Layout::Newline)
            || self.suite_closed && !self.suite_inline
            || self.at_layout(Layout::Dedent)
            || self.at_layout(Layout::Indent)
            || self.at_eof();
        if !ended {
            self.expected("a line ending");
            self.recover_line();
            self.eat_layout(Layout::Newline);
        }
        if self.stmt_errored {
            self.skip_fallout();
        }
    }

    /// The rest of a statement that already reported: the indented body
    /// its header would have opened and the `else` lines that continue
    /// it, skipped without a second report. A later statement at this
    /// level reports its own errors.
    fn skip_fallout(&mut self) {
        loop {
            if self.at_layout(Layout::Indent) {
                self.skip_indented();
            } else if self.at(TokenKind::KwElse) {
                self.recover_line();
                self.eat_layout(Layout::Newline);
            } else {
                return;
            }
        }
    }

    /// Skips an indented region, `INDENT` to its `DEDENT`, into an `Error`
    /// node.
    fn skip_indented(&mut self) {
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
                Some(Layout::Newline) | None => {}
            }
            if self.at_eof() {
                break;
            }
            self.bump();
        }
        self.complete(marker, SyntaxKind::Error);
        self.suite_closed = true;
    }

    /// An `INDENT` no suite opened: one error, which is its first line's,
    /// then its lines are read as part of the current suite.
    pub(crate) fn stray_indent(&mut self) {
        self.stmt_errored = false;
        self.error(Code::SyntaxError, "unexpected indentation");
        let marker = self.start();
        self.bump();
        self.statements(true);
        self.eat_layout(Layout::Dedent);
        self.complete(marker, SyntaxKind::Error);
    }

    // ----------------------------------------------------------- nesting

    pub(crate) fn enter(&mut self) -> bool {
        if self.nesting >= MAX_NESTING {
            let at = self.pos();
            if !self.errors.iter().any(|e| e.code == Code::NestingTooDeep) {
                self.report(Code::NestingTooDeep, at, NESTING_TOO_DEEP);
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

    /// `grammar.limit.chain`: past `MAX_CHAIN_LINKS`, reports the link
    /// that began at token `at` and skips the rest of the chain into an
    /// error node around `done`, so no tool walks a deeper tree.
    pub(crate) fn chain_too_long(&mut self, done: Done, links: u32, at: usize) -> Option<Done> {
        if links <= MAX_CHAIN_LINKS {
            return None;
        }
        self.chain_past_limit(at);
        let marker = self.precede(done);
        self.skip_balanced();
        Some(self.complete(marker, SyntaxKind::Error))
    }

    /// `grammar.limit.chain.error`: the link at token `at` is the first
    /// past the limit. Reports it, once per file as every nesting error.
    pub(crate) fn chain_past_limit(&mut self, at: usize) {
        if !self.errors.iter().any(|e| e.code == Code::NestingTooDeep) {
            self.report(Code::NestingTooDeep, at, CHAIN_TOO_LONG);
        }
        self.stmt_errored = true;
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
        let column = crate::docs::column_of(self.src, self.toks, start);
        for comment in crate::docs::attached_above(self.src, self.toks, start, column) {
            self.attached[comment] = true;
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
                    "documentation comments must attach to a declaration or member",
                ));
            }
        }
    }
}

/// A tab at `at` inside a line's indentation: the span from it through the
/// line's first token, whose column the tab left undefined. The layout read
/// there (a missing suite, a stray indent, a dedent to no level) is the
/// tab's fallout, not a second error.
fn tabbed_indentation(src: &str, at: u32) -> Option<(u32, u32)> {
    let bytes = src.as_bytes();
    let at = usize::try_from(at).ok()?;
    let blank = |byte: &u8| matches!(byte, b' ' | b'\t');
    let line = &bytes[..at.min(bytes.len())];
    if !line
        .iter()
        .rev()
        .take_while(|byte| !matches!(byte, b'\n' | b'\r'))
        .all(blank)
    {
        return None;
    }
    let first = at
        + bytes
            .get(at..)?
            .iter()
            .take_while(|byte| blank(byte))
            .count();
    Some((u32_of(at), u32_of(first + 1)))
}

/// The scope of each `unclosed-delimiter` the lexer reported: from its
/// opener to the next column-0 item start, else to the end of the file.
fn unclosed_scopes(toks: &TokenBuf, lexed: &[Diagnostic]) -> Vec<(u32, u32)> {
    lexed
        .iter()
        .filter(|diagnostic| diagnostic.code == Code::UnclosedDelimiter)
        .map(|diagnostic| {
            let lo = diagnostic.primary.lo;
            let end = toks
                .resync
                .iter()
                .map(|&token| toks.start[token as usize])
                .find(|&start| start > lo)
                .unwrap_or(u32::MAX);
            (lo, end)
        })
        .collect()
}

fn diag(code: Code, lo: u32, hi: u32, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        code,
        Span {
            file: FileId::from_raw(0),
            lo,
            hi,
        },
        message,
    )
}

/// The top-level children that hold a parse error (`grammar.recovery.item`).
/// An error a top-level item's parse raised is that item's, wherever it
/// stands: `enum impl` reports the missing enum name at `impl`, the next
/// child's first token, and leaves an `EnumDecl` with no name, while the
/// next item may be whole. `errored` lists those items' first tokens, in
/// order. Any other diagnostic (the lexer's, layout's, a doc comment's, or
/// one raised between items) belongs to the child whose source holds it:
/// each child owns the source from its first token to the next child's
/// first token, so one at a line end or at the end of the file belongs to
/// the item it ends. `placed` holds those diagnostics' starts, sorted. A
/// child that is junk (`ERROR`) is broken too.
/// One pass over the root's children, with a binary search each, and only
/// when the parse reported something.
fn broken_items(
    tree: &GreenTree,
    tokens: &TokenBuf,
    placed: &[u32],
    errored: &[u32],
) -> Vec<NodeIdx> {
    if placed.is_empty() && errored.is_empty() {
        return Vec::new();
    }
    let starts: Vec<(NodeIdx, u32, u32)> = tree
        .root()
        .children()
        .filter_map(|child| {
            // A node with no token has a `NONE` first token.
            let (first, _) = tree.span_tokens(child.index());
            let first = first.raw();
            ((first as usize) < tokens.len())
                .then(|| (child.index(), first, tokens.start[first as usize]))
        })
        .collect();
    let mut out = Vec::new();
    for (i, &(node, first, lo)) in starts.iter().enumerate() {
        let hi = starts.get(i + 1).map_or(u32::MAX, |&(_, _, next)| next);
        let at = placed.partition_point(|&p| p < lo);
        // Junk a line's recovery skipped is never an item to check.
        if errored.binary_search(&first).is_ok()
            || placed.get(at).is_some_and(|&p| p < hi)
            || tree.node(node).kind() == SyntaxKind::Error
        {
            out.push(node);
        }
    }
    out
}

fn build_item_index(tree: &GreenTree, tokens: &TokenBuf, source: &str) -> (ItemIndex, Interner) {
    let mut result = ItemIndex::default();
    let mut symbols = Interner::new();
    for child in tree.root().children() {
        let kind = match child.kind() {
            SyntaxKind::FnDecl => ItemKind::Function,
            SyntaxKind::StructDecl => ItemKind::Struct,
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
