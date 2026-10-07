use std::collections::VecDeque;

use hd_base::TokenIdx;

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

#[derive(Clone, Copy, Debug)]
struct OpenSuite {
    same_line: bool,
    depth: u16,
}

pub struct LayoutCursor<'t> {
    tokens: &'t TokenBuf,
    pos: usize,
    depth: u16,
    indents: Vec<u16>,
    suites: Vec<OpenSuite>,
    pending: VecDeque<Layout>,
    prepared: bool,
    emitted_final_newline: bool,
    previous_line: Option<usize>,
    logical_indent: u16,
}

impl<'t> LayoutCursor<'t> {
    #[must_use]
    pub fn new(tokens: &'t TokenBuf) -> Self {
        Self {
            tokens,
            pos: 0,
            depth: 0,
            indents: vec![0],
            suites: Vec::new(),
            pending: VecDeque::new(),
            prepared: false,
            emitted_final_newline: false,
            previous_line: None,
            logical_indent: 0,
        }
    }

    pub fn peek(&mut self) -> TokenOrLayout {
        self.prepare();
        if let Some(&kind) = self.pending.front() {
            return TokenOrLayout::Layout {
                kind,
                at: TokenIdx::from_raw(self.pos_u32()),
            };
        }
        if self.pos < self.tokens.len() {
            TokenOrLayout::Token(TokenIdx::from_raw(self.pos_u32()))
        } else {
            TokenOrLayout::Eof
        }
    }

    pub fn bump(&mut self) {
        self.prepare();
        if self.pending.pop_front().is_some() {
            return;
        }
        if self.pos >= self.tokens.len() {
            return;
        }
        match self.tokens.kind[self.pos] {
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => {
                self.depth = self.depth.saturating_add(1);
            }
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                self.depth = self.depth.saturating_sub(1);
            }
            _ => {}
        }
        self.previous_line = Some(self.tokens.line_of(self.tokens.start[self.pos]));
        self.pos += 1;
        self.prepared = false;
    }

    /// Records that the colon most recently consumed by the parser opens a suite.
    pub fn open_suite(&mut self, _closure: bool) {
        let colon_line = self.previous_line.unwrap_or(0);
        let next_line = self
            .tokens
            .start
            .get(self.pos)
            .map(|&offset| self.tokens.line_of(offset));
        self.suites.push(OpenSuite {
            same_line: next_line == Some(colon_line),
            depth: self.depth,
        });
        self.prepared = false;
    }

    fn prepare(&mut self) {
        if self.prepared || !self.pending.is_empty() {
            return;
        }
        self.prepared = true;
        if self.pos == self.tokens.len() {
            while self.suites.last().is_some_and(|suite| suite.same_line) {
                self.suites.pop();
                self.pending.push_back(Layout::SuiteEnd);
            }
            if !self.emitted_final_newline && self.previous_line.is_some() {
                self.pending.push_back(Layout::Newline);
                self.emitted_final_newline = true;
            }
            while self.indents.len() > 1 {
                self.indents.pop();
                self.pending.push_back(Layout::Dedent);
            }
            return;
        }

        if self
            .tokens
            .is_line_first(TokenIdx::from_raw(self.pos_u32()))
        {
            let line = self.tokens.line_of(self.tokens.start[self.pos]);
            if self.previous_line.is_some_and(|previous| previous != line)
                && !self.continues_previous(line)
            {
                while self.suites.last().is_some_and(|suite| suite.same_line) {
                    self.suites.pop();
                    self.pending.push_back(Layout::SuiteEnd);
                }
                if self.depth == 0 || self.suites.last().is_some_and(|suite| !suite.same_line) {
                    self.pending.push_back(Layout::Newline);
                    let indent = self.tokens.line_indent[line];
                    match indent.cmp(self.indents.last().expect("indent stack is never empty")) {
                        std::cmp::Ordering::Greater => {
                            self.indents.push(indent);
                            self.pending.push_back(Layout::Indent);
                        }
                        std::cmp::Ordering::Less => {
                            while self.indents.last().is_some_and(|&active| active > indent) {
                                self.indents.pop();
                                self.pending.push_back(Layout::Dedent);
                            }
                        }
                        std::cmp::Ordering::Equal => {}
                    }
                    self.logical_indent = indent;
                }
            } else if self.previous_line.is_none() {
                self.logical_indent = self.tokens.line_indent[line];
            }
        }

        let kind = self.tokens.kind[self.pos];
        if matches!(
            kind,
            TokenKind::Comma | TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace
        ) {
            while self
                .suites
                .last()
                .is_some_and(|suite| suite.same_line && suite.depth == self.depth)
            {
                self.suites.pop();
                self.pending.push_back(Layout::SuiteEnd);
            }
        }
        if kind == TokenKind::KwElse && self.suites.last().is_some_and(|suite| suite.same_line) {
            self.suites.pop();
            self.pending.push_back(Layout::SuiteEnd);
        }
    }

    fn continues_previous(&self, line: usize) -> bool {
        if self.depth > 0 && self.suites.last().is_none_or(|suite| suite.same_line) {
            return true;
        }
        let kind = self.tokens.kind[self.pos];
        matches!(kind, TokenKind::Dot | TokenKind::PipeGt)
            && self.tokens.line_indent[line] > self.logical_indent
    }

    fn pos_u32(&self) -> u32 {
        self.pos.try_into().unwrap_or(u32::MAX - 1)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Layout, LayoutCursor, TokenKind, TokenOrLayout, lex};

    #[test]
    fn emits_depth_zero_layout() {
        let lexed = lex(b"fn f():\n    +1\nnext := +2\n");
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
        assert!(
            layouts
                .windows(2)
                .any(|pair| pair == [Layout::Newline, Layout::Indent])
        );
        assert!(layouts.contains(&Layout::Dedent));
    }
}
