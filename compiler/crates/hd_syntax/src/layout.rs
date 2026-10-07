//! The layout cursor (syntax.md §4.2): the parser pulls tokens through it,
//! and it inserts the abstract `NEWLINE`, `INDENT` and `DEDENT` tokens of
//! [Physical And Logical Lines](spec/lang/01-lexical-structure.md). The
//! parser tells it when a colon or `=>` opens a suite; everything else is
//! the indentation stack, the bracket depth and the continuation rules.
//! Same-line suites end where the parser's inline statement ends, and the
//! parser records their `SUITE_END`.

use std::collections::VecDeque;

use hd_base::TokenIdx;
use hd_diag::Code;

use crate::{TokenBuf, TokenKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Layout {
    Newline,
    Indent,
    Dedent,
    SuiteEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenOrLayout {
    Token(TokenIdx),
    Layout { kind: Layout, at: TokenIdx },
    Eof,
}

/// How a suite opened by [`LayoutCursor::open_suite`] continues.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuiteKind {
    SameLine,
    Indented,
    /// The next line is not indented farther: the suite has no body.
    Missing,
}

#[derive(Clone, Copy, Debug)]
struct Block {
    /// Column of the body's statements.
    indent: u16,
    /// Bracket depth of the body's statements.
    depth: u16,
    /// Column of the physical line holding the header (`lex.nested.reference`).
    header: u16,
    closure: bool,
    /// Opened inside brackets that its parent block's statement opened.
    in_brackets: bool,
    /// Column of the first physical line of the current logical line.
    logical: u16,
    /// The current logical line holds `|>` at this depth (`lex.pipe.no-dot-line`).
    piped: bool,
    /// An indentation the parser did not open (recovery for a stray indent).
    stray: bool,
}

pub struct LayoutCursor<'t> {
    tokens: &'t TokenBuf,
    pos: usize,
    depth: u16,
    blocks: Vec<Block>,
    /// Bracket depths of the open same-line suites, innermost last.
    inline: Vec<u16>,
    pending: VecDeque<Layout>,
    /// The line-first token whose layout is already queued.
    prepared: usize,
    opening: Option<bool>,
    eof: bool,
    pub(crate) errors: Vec<(Code, usize)>,
}

impl<'t> LayoutCursor<'t> {
    #[must_use]
    pub fn new(tokens: &'t TokenBuf) -> Self {
        Self {
            tokens,
            pos: 0,
            depth: 0,
            blocks: vec![Block {
                indent: 0,
                depth: 0,
                header: 0,
                closure: false,
                in_brackets: false,
                logical: 0,
                piped: false,
                stray: false,
            }],
            inline: Vec::new(),
            pending: VecDeque::new(),
            prepared: usize::MAX,
            opening: None,
            eof: false,
            errors: Vec::new(),
        }
    }

    /// The token index the cursor stands at.
    #[must_use]
    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn peek(&mut self) -> TokenOrLayout {
        self.prepare();
        let at = TokenIdx::from_raw(u32::try_from(self.pos).unwrap_or(u32::MAX - 1));
        if let Some(&kind) = self.pending.front() {
            return TokenOrLayout::Layout { kind, at };
        }
        if self.pos < self.tokens.len() {
            TokenOrLayout::Token(at)
        } else {
            TokenOrLayout::Eof
        }
    }

    /// The layout queued before the next token, without consuming it.
    pub fn pending(&mut self) -> Vec<Layout> {
        self.prepare();
        self.pending.iter().copied().collect()
    }

    pub fn bump(&mut self) {
        self.prepare();
        if self.pending.pop_front().is_some() {
            return;
        }
        if self.pos >= self.tokens.len() {
            return;
        }
        let kind = self.tokens.kind[self.pos];
        if kind.opens() {
            self.depth = self.depth.saturating_add(1);
        } else if kind.closes() {
            self.depth = self.depth.saturating_sub(1);
        }
        if kind == TokenKind::PipeGt
            && let Some(block) = self.blocks.last_mut()
            && block.depth == self.depth
        {
            block.piped = true;
        }
        self.pos += 1;
    }

    /// Called right after the parser consumes a suite-introducing `:` (or a
    /// match arm's `=>`). A token on the same line starts a same-line suite.
    pub fn open_suite(&mut self, closure: bool) -> SuiteKind {
        debug_assert!(self.pending.is_empty());
        if self.pos < self.tokens.len() && !self.line_first(self.pos) {
            self.inline.push(self.depth);
            return SuiteKind::SameLine;
        }
        let block = *self.blocks.last().expect("root block");
        let header = self.header_column();
        let need = header.max(block.logical);
        if self.pos >= self.tokens.len() || self.column(self.pos) <= need {
            if self.depth > block.depth
                && self.pos < self.tokens.len()
                && !self.tokens.kind[self.pos].closes()
                && self.tokens.kind[self.pos] != TokenKind::Comma
            {
                // lex.nested.body-depth: the body is still read as one.
                self.errors.push((Code::UnexpectedIndentation, self.pos));
                self.opening = Some(closure);
                self.force_open(header);
                return SuiteKind::Indented;
            }
            return SuiteKind::Missing;
        }
        self.opening = Some(closure);
        self.force_open(header);
        SuiteKind::Indented
    }

    /// After a match arm's `=>`: an indented body, or a statement on the
    /// same line (no same-line suite, so no `SUITE_END`).
    pub fn open_arm(&mut self) -> SuiteKind {
        let kind = self.open_suite(false);
        if kind == SuiteKind::SameLine {
            self.inline.pop();
        }
        kind
    }

    /// The parser finished a same-line suite body.
    pub fn close_inline(&mut self) {
        self.inline.pop();
    }

    fn force_open(&mut self, header: u16) {
        let closure = self.opening.take().unwrap_or(false);
        let indent = self.column(self.pos);
        let in_brackets = self
            .blocks
            .last()
            .is_some_and(|parent| self.depth > parent.depth);
        self.blocks.push(Block {
            indent,
            depth: self.depth,
            header,
            closure,
            in_brackets,
            logical: indent,
            piped: false,
            stray: false,
        });
        self.prepared = self.pos;
        self.pending.push_back(Layout::Newline);
        self.pending.push_back(Layout::Indent);
    }

    fn header_column(&self) -> u16 {
        let colon = self.pos.saturating_sub(1);
        let offset = self.tokens.start.get(colon).copied().unwrap_or(0);
        let mut line = self.tokens.line_of(offset);
        // A header on a line that continues a string: walk to its first line.
        while line > 0 && self.tokens.line_tok[line].get().is_none() {
            line -= 1;
        }
        self.tokens.line_indent[line]
    }

    fn column(&self, token: usize) -> u16 {
        let line = self.tokens.line_of(self.tokens.start[token]);
        self.tokens.line_indent[line]
    }

    fn line_first(&self, token: usize) -> bool {
        self.tokens
            .is_line_first(TokenIdx::from_raw(u32::try_from(token).unwrap_or(0)))
    }

    fn prepare(&mut self) {
        if !self.pending.is_empty() {
            return;
        }
        if self.pos >= self.tokens.len() {
            if !self.eof {
                self.eof = true;
                if self.pos > 0 {
                    self.pending.push_back(Layout::Newline);
                }
                while self.blocks.len() > 1 {
                    self.blocks.pop();
                    self.pending.push_back(Layout::Dedent);
                }
            }
            return;
        }
        let kind = self.tokens.kind[self.pos];
        if self.prepared != self.pos && self.line_first(self.pos) && self.pos > 0 {
            self.prepared = self.pos;
            self.line_start(kind);
            if !self.pending.is_empty() {
                return;
            }
        }
        if kind.closes() || kind == TokenKind::StrMid {
            self.close_nested_at_closer();
        }
    }

    /// `lex.nested.closer`: a closer at a nested suite's depth ends it.
    fn close_nested_at_closer(&mut self) {
        let mut newline = false;
        while self.blocks.len() > 1 {
            let block = *self.blocks.last().expect("block");
            if block.depth != self.depth || !block.in_brackets {
                break;
            }
            if block.closure && self.prepared != self.pos {
                // lex.closure.closer-on-body.
                self.errors.push((Code::SyntaxError, self.pos));
            }
            if !newline {
                self.pending.push_back(Layout::Newline);
                newline = true;
            }
            self.pending.push_back(Layout::Dedent);
            self.blocks.pop();
        }
    }

    fn line_start(&mut self, kind: TokenKind) {
        let column = self.column(self.pos);
        let mut newline = false;
        loop {
            let block = *self.blocks.last().expect("root block");
            if self.depth > block.depth {
                return;
            }
            if column >= block.indent || self.blocks.len() == 1 {
                if self.continues(kind, column, block) {
                    return;
                }
                if !newline {
                    self.pending.push_back(Layout::Newline);
                }
                if column > block.indent {
                    // An indentation no suite opened: INDENT, then the
                    // parser reports it (spec `lex.indent.greater`).
                    self.blocks.push(Block {
                        indent: column,
                        depth: self.depth,
                        header: block.indent,
                        closure: false,
                        in_brackets: false,
                        logical: column,
                        piped: false,
                        stray: true,
                    });
                    self.pending.push_back(Layout::Indent);
                    return;
                }
                let block = self.blocks.last_mut().expect("block");
                block.logical = column;
                block.piped = false;
                return;
            }
            // Dedent out of `block`.
            let nested = block.in_brackets;
            if nested && column > block.header && !block.stray {
                // A line between the header and the body.
                let code = if block.closure {
                    Code::SyntaxError
                } else {
                    Code::InvalidDedent
                };
                self.errors.push((code, self.pos));
            }
            if !newline {
                self.pending.push_back(Layout::Newline);
                newline = true;
            }
            self.pending.push_back(Layout::Dedent);
            self.blocks.pop();
            if block.closure
                && nested
                && self.depth == block.depth
                && !(kind.closes() || kind == TokenKind::Comma)
            {
                // lex.closure.end-token.
                self.errors.push((Code::SyntaxError, self.pos));
            }
            let parent = *self.blocks.last().expect("root block");
            if !nested && parent.depth == self.depth && column > parent.indent {
                // lex.indent.unknown-column: go on at the parent's level.
                self.errors.push((Code::InvalidDedent, self.pos));
                let parent = self.blocks.last_mut().expect("block");
                parent.logical = column;
                parent.piped = false;
                return;
            }
        }
    }

    /// Leading-dot and leading-pipe continuation (`lex.dot.*`, `lex.pipe.*`).
    fn continues(&mut self, kind: TokenKind, column: u16, block: Block) -> bool {
        let dot = kind == TokenKind::Dot
            && matches!(
                self.tokens.kind.get(self.pos + 1),
                Some(TokenKind::Ident | TokenKind::RawIdent)
            );
        if !(dot || kind == TokenKind::PipeGt) || column <= block.logical {
            return false;
        }
        let previous = self.tokens.kind[self.pos - 1];
        if matches!(previous, TokenKind::Colon | TokenKind::FatArrow) {
            return false;
        }
        if self.inline.last().is_some_and(|&depth| depth == self.depth) || dot && block.piped {
            // lex.dot.open-suite, lex.pipe.open-suite, lex.pipe.no-dot-line
            self.errors.push((Code::SyntaxError, self.pos));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::{Layout, LayoutCursor, TokenKind, TokenOrLayout, lex};

    fn layouts(source: &str) -> Vec<Layout> {
        let lexed = lex(source.as_bytes());
        let mut cursor = LayoutCursor::new(&lexed.tokens);
        let mut layouts = Vec::new();
        loop {
            match cursor.peek() {
                TokenOrLayout::Token(token) => {
                    let kind = lexed.tokens.kind(token);
                    cursor.bump();
                    if kind == TokenKind::Colon {
                        cursor.open_suite(false);
                    }
                }
                TokenOrLayout::Layout { kind, .. } => {
                    layouts.push(kind);
                    cursor.bump();
                }
                TokenOrLayout::Eof => break,
            }
        }
        layouts
    }

    #[test]
    fn emits_depth_zero_layout() {
        let layouts = layouts("fn f():\n    +1\nnext := +2\n");
        assert_eq!(
            layouts,
            [
                Layout::Newline,
                Layout::Indent,
                Layout::Newline,
                Layout::Dedent,
                Layout::Newline
            ]
        );
    }

    #[test]
    fn bracket_continuation_suppresses_layout() {
        let layouts = layouts("values := [\n    +1,\n    +2,\n]\nnext := +3\n");
        assert_eq!(layouts, [Layout::Newline, Layout::Newline]);
    }
}
