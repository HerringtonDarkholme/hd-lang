use hd_base::{FileId, Span, TokenIdx};
use hd_diag::{Code, Diagnostic};
use unicode_ident::{is_xid_continue, is_xid_start};
use unicode_normalization::UnicodeNormalization;

const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TokenKind {
    Error,
    Ident,
    RawIdent,
    Placeholder,
    Number,
    String,
    Char,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Dot,
    Colon,
    Semicolon,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    StarStar,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    AndAnd,
    OrOr,
    PipeGt,
    Eq,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    ColonEq,
    Arrow,
    FatArrow,
    Question,
    Bang,
    Dollar,
    At,
    Ellipsis,
    EllipsisEq,
    ColonColon,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    AmpEq,
    PipeEq,
    CaretEq,
    ShlEq,
    ShrEq,
    DotDot,
    DotDotEq,
    KwSelfType,
    KwBreak,
    KwContinue,
    KwData,
    KwDefer,
    KwDyn,
    KwElse,
    KwEnum,
    KwFalse,
    KwFn,
    KwFor,
    KwIf,
    KwImpl,
    KwIn,
    KwIs,
    KwLet,
    KwMatch,
    KwMut,
    KwPass,
    KwPub,
    KwReturn,
    KwSelfValue,
    KwTests,
    KwTrait,
    KwTrue,
    KwType,
    KwWhile,
}

const _: () = assert!(core::mem::size_of::<TokenKind>() == 1);

impl TokenKind {
    #[must_use]
    pub fn is_keyword(self) -> bool {
        self as u8 >= Self::KwSelfType as u8
    }

    #[must_use]
    pub const fn is_open_delimiter(self) -> bool {
        matches!(self, Self::LParen | Self::LBracket | Self::LBrace)
    }

    #[must_use]
    pub const fn is_close_delimiter(self) -> bool {
        matches!(self, Self::RParen | Self::RBracket | Self::RBrace)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CommentKind {
    Plain,
    Doc,
    ModuleDoc,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct LineFlags(pub u8);

impl LineFlags {
    pub const NON_ASCII: u8 = 1;
    pub const HAS_TAB: u8 = 2;
    pub const CONTINUES_STRING: u8 = 4;
}

#[derive(Clone, Debug, Default)]
pub struct TokenBuf {
    pub kind: Vec<TokenKind>,
    pub start: Vec<u32>,
    pub end: Vec<u32>,
    pub line_first: Vec<u32>,
    pub line_start: Vec<u32>,
    pub line_tok: Vec<TokenIdx>,
    pub line_indent: Vec<u16>,
    pub line_flags: Vec<LineFlags>,
    pub com_start: Vec<u32>,
    pub com_end: Vec<u32>,
    pub com_kind: Vec<CommentKind>,
}

impl TokenBuf {
    #[must_use]
    pub fn len(&self) -> usize {
        self.kind.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.kind.is_empty()
    }

    #[must_use]
    pub fn kind(&self, token: TokenIdx) -> TokenKind {
        self.kind[token.idx()]
    }

    #[must_use]
    pub fn span(&self, token: TokenIdx) -> (u32, u32) {
        (self.start[token.idx()], self.end[token.idx()])
    }

    #[must_use]
    pub fn text<'s>(&self, token: TokenIdx, source: &'s str) -> &'s str {
        let (start, end) = self.span(token);
        &source[start as usize..end as usize]
    }

    #[must_use]
    pub fn is_line_first(&self, token: TokenIdx) -> bool {
        let raw = token.raw() as usize;
        self.line_first
            .get(raw / 32)
            .is_some_and(|word| word & (1 << (raw % 32)) != 0)
    }

    #[must_use]
    pub fn line_of(&self, offset: u32) -> usize {
        self.line_start
            .partition_point(|&start| start <= offset)
            .saturating_sub(1)
    }

    #[must_use]
    pub fn reconstruct(&self, source: &str) -> String {
        let mut output = String::with_capacity(source.len());
        let mut previous = 0;
        for (&start, &end) in self.start.iter().zip(&self.end) {
            let start = start as usize;
            let end = end as usize;
            output.push_str(&source[previous..start]);
            output.push_str(&source[start..end]);
            previous = end;
        }
        output.push_str(&source[previous..]);
        output
    }

    fn push(&mut self, kind: TokenKind, start: usize, end: usize, line: usize) {
        let token = self.kind.len();
        self.kind.push(kind);
        self.start.push(to_u32(start));
        self.end.push(to_u32(end));
        while self.line_first.len() <= token / 32 {
            self.line_first.push(0);
        }
        if self.line_tok[line].get().is_none() {
            self.line_tok[line] = TokenIdx::from_raw(to_u32(token));
            self.line_first[token / 32] |= 1 << (token % 32);
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Lexed {
    pub tokens: TokenBuf,
    pub diagnostics: Vec<Diagnostic>,
}

#[must_use]
pub fn lex(source: &[u8]) -> Lexed {
    let Ok(text) = core::str::from_utf8(source) else {
        return Lexed {
            diagnostics: vec![diagnostic(Code::InvalidToken, 0, source.len())],
            ..Lexed::default()
        };
    };
    let mut lexer = Lexer::new(text);
    lexer.run();
    Lexed {
        tokens: lexer.tokens,
        diagnostics: lexer.diagnostics,
    }
}

struct Lexer<'s> {
    source: &'s str,
    bytes: &'s [u8],
    pos: usize,
    line: usize,
    tokens: TokenBuf,
    diagnostics: Vec<Diagnostic>,
}

impl<'s> Lexer<'s> {
    fn new(source: &'s str) -> Self {
        let mut tokens = TokenBuf::default();
        build_lines(source.as_bytes(), &mut tokens);
        let token_capacity = source.len() / 4;
        tokens.kind.reserve(token_capacity);
        tokens.start.reserve(token_capacity);
        tokens.end.reserve(token_capacity);
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            line: 0,
            tokens,
            diagnostics: Vec::new(),
        }
    }

    fn run(&mut self) {
        if self.bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            self.pos = 3;
        }
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' => self.pos += 1,
                b'\t' => {
                    self.error(Code::TabWhitespace, self.pos, self.pos + 1);
                    self.pos += 1;
                }
                b'\n' => {
                    self.pos += 1;
                    self.line += 1;
                }
                b'\r' if self.peek_byte(1) == Some(b'\n') => {
                    self.pos += 2;
                    self.line += 1;
                }
                b'\r' => {
                    self.error(Code::InvalidToken, self.pos, self.pos + 1);
                    self.pos += 1;
                    self.line += 1;
                }
                b'#' => self.comment(),
                b'`' => self.raw_identifier(),
                b'"' => self.string(self.pos, false),
                b'\'' => self.character(),
                b'0'..=b'9' => self.number(),
                byte if is_ident_start_byte(byte, self.source, self.pos) => self.identifier(),
                _ => self.punctuation_or_error(),
            }
        }
    }

    fn comment(&mut self) {
        let start = self.pos;
        let doc = self.peek_byte(1) == Some(b'#');
        while self.pos < self.bytes.len() && !matches!(self.bytes[self.pos], b'\n' | b'\r') {
            self.pos += 1;
        }
        let module_doc = doc
            && self.tokens.kind.is_empty()
            && self.line + 1 < self.tokens.line_start.len()
            && line_is_blank(self.bytes, self.line + 1, &self.tokens.line_start);
        self.tokens.com_start.push(to_u32(start));
        self.tokens.com_end.push(to_u32(self.pos));
        self.tokens.com_kind.push(if module_doc {
            CommentKind::ModuleDoc
        } else if doc {
            CommentKind::Doc
        } else {
            CommentKind::Plain
        });
    }

    fn raw_identifier(&mut self) {
        let start = self.pos;
        self.pos += 1;
        let content_start = self.pos;
        if !self.consume_identifier_chars() || self.peek_byte(0) != Some(b'`') {
            while self.pos < self.bytes.len()
                && !matches!(self.bytes[self.pos], b'`' | b'\n' | b'\r')
            {
                self.pos += char_len(self.source, self.pos);
            }
            if self.peek_byte(0) == Some(b'`') {
                self.pos += 1;
            }
            self.error(Code::InvalidToken, start, self.pos);
            self.tokens
                .push(TokenKind::Error, start, self.pos, self.line);
            return;
        }
        let content_end = self.pos;
        self.pos += 1;
        let content = &self.source[content_start..content_end];
        if content == "_" || !is_nfc(content) {
            self.error(Code::InvalidToken, start, self.pos);
        }
        self.tokens
            .push(TokenKind::RawIdent, start, self.pos, self.line);
    }

    fn identifier(&mut self) {
        let start = self.pos;
        self.consume_identifier_chars();
        if self.peek_byte(0) == Some(b'"') {
            self.string(start, true);
            return;
        }
        let text = &self.source[start..self.pos];
        let kind = keyword(text).unwrap_or_else(|| {
            if text == "_" {
                TokenKind::Placeholder
            } else {
                TokenKind::Ident
            }
        });
        if !is_nfc(text) {
            self.error(Code::InvalidToken, start, self.pos);
        }
        self.tokens.push(kind, start, self.pos, self.line);
    }

    fn consume_identifier_chars(&mut self) -> bool {
        let start = self.pos;
        let Some(first) = self.source[self.pos..].chars().next() else {
            return false;
        };
        if first != '_' && !is_xid_start(first) {
            return false;
        }
        self.pos += first.len_utf8();
        for ch in self.source[self.pos..].chars() {
            if ch == '_' || is_xid_continue(ch) {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        self.pos > start
    }

    fn number(&mut self) {
        let start = self.pos;
        let mut radix = 10;
        if self.peek_byte(0) == Some(b'0') {
            radix = match self.peek_byte(1) {
                Some(b'b' | b'B') => 2,
                Some(b'o' | b'O') => 8,
                Some(b'x' | b'X') => 16,
                _ => 10,
            };
            if radix != 10 {
                self.pos += 2;
            }
        }
        let mut bad_separator = false;
        let mut previous_separator = false;
        while let Some(byte) = self.peek_byte(0) {
            if digit_for_radix(byte, radix) {
                previous_separator = false;
                self.pos += 1;
            } else if byte == b'_' {
                bad_separator |= previous_separator;
                previous_separator = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        bad_separator |= previous_separator;
        if radix == 10 {
            if self.peek_byte(0) == Some(b'.')
                && self.peek_byte(1).is_some_and(|b| b.is_ascii_digit())
            {
                self.pos += 1;
                self.consume_decimal_digits(&mut bad_separator);
            }
            if matches!(self.peek_byte(0), Some(b'e' | b'E')) {
                let exponent = self.pos;
                let mut look = self.pos + 1;
                if self
                    .bytes
                    .get(look)
                    .is_some_and(|b| matches!(b, b'+' | b'-'))
                {
                    look += 1;
                }
                if self.bytes.get(look).is_some_and(u8::is_ascii_digit) {
                    self.pos = look;
                    self.consume_decimal_digits(&mut bad_separator);
                } else {
                    self.pos = exponent;
                }
            }
            if self.source[self.pos..]
                .chars()
                .next()
                .is_some_and(is_xid_start)
            {
                self.consume_identifier_chars();
            }
        } else if self.source[self.pos..]
            .chars()
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_alphanumeric())
        {
            while self.source[self.pos..]
                .chars()
                .next()
                .is_some_and(|ch| ch == '_' || ch.is_alphanumeric())
            {
                self.pos += char_len(self.source, self.pos);
            }
            self.error(Code::SyntaxError, start, self.pos);
        }
        if bad_separator {
            self.error(Code::InvalidToken, start, self.pos);
        }
        self.tokens
            .push(TokenKind::Number, start, self.pos, self.line);
    }

    fn consume_decimal_digits(&mut self, bad: &mut bool) {
        let mut previous_separator = false;
        while let Some(byte) = self.peek_byte(0) {
            if byte.is_ascii_digit() {
                previous_separator = false;
                self.pos += 1;
            } else if byte == b'_' {
                *bad |= previous_separator;
                previous_separator = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        *bad |= previous_separator;
    }

    fn string(&mut self, start: usize, prefixed: bool) {
        debug_assert_eq!(self.peek_byte(0), Some(b'"'));
        let multiline = self.bytes.get(self.pos..self.pos.saturating_add(3)) == Some(b"\"\"\"");
        let delimiter = if multiline { 3 } else { 1 };
        self.pos += delimiter;
        let start_line = self.line;
        let mut escaped = false;
        let mut closed = false;
        while self.pos < self.bytes.len() {
            if !escaped
                && self.bytes.get(self.pos..self.pos + delimiter) == Some(&b"\"\"\""[..delimiter])
            {
                self.pos += delimiter;
                closed = true;
                break;
            }
            let byte = self.bytes[self.pos];
            if !multiline && matches!(byte, b'\n' | b'\r') {
                break;
            }
            if byte == b'\n' {
                self.line += 1;
            } else if byte == b'\r' && self.peek_byte(1) == Some(b'\n') {
                self.line += 1;
                self.pos += 1;
            }
            if !prefixed && byte == b'\\' && !escaped {
                if !valid_escape(self.bytes, self.pos) {
                    self.error(
                        Code::InvalidEscape,
                        self.pos,
                        (self.pos + 2).min(self.bytes.len()),
                    );
                }
                escaped = true;
            } else {
                escaped = prefixed && byte == b'\\' && !escaped;
            }
            self.pos += 1;
        }
        if !closed {
            self.error(Code::UnterminatedString, start, self.pos);
        }
        if self.line > start_line {
            for line in (start_line + 1)
                ..=self
                    .line
                    .min(self.tokens.line_flags.len().saturating_sub(1))
            {
                self.tokens.line_flags[line].0 |= LineFlags::CONTINUES_STRING;
            }
        }
        self.tokens
            .push(TokenKind::String, start, self.pos, start_line);
    }

    fn character(&mut self) {
        let start = self.pos;
        self.pos += 1;
        let mut escaped = false;
        let mut closed = false;
        while self.pos < self.bytes.len() {
            let byte = self.bytes[self.pos];
            if matches!(byte, b'\n' | b'\r') {
                break;
            }
            if byte == b'\'' && !escaped {
                self.pos += 1;
                closed = true;
                break;
            }
            if byte == b'\\' && !escaped {
                if !valid_escape(self.bytes, self.pos) {
                    self.error(
                        Code::InvalidEscape,
                        self.pos,
                        (self.pos + 2).min(self.bytes.len()),
                    );
                }
                escaped = true;
            } else {
                escaped = false;
            }
            self.pos += char_len(self.source, self.pos);
        }
        if !closed {
            self.error(Code::UnterminatedString, start, self.pos);
        }
        self.tokens
            .push(TokenKind::Char, start, self.pos, self.line);
    }

    fn punctuation_or_error(&mut self) {
        let start = self.pos;
        let candidates: &[(&[u8], TokenKind)] = &[
            (b"...=", TokenKind::EllipsisEq),
            (b"<<=", TokenKind::ShlEq),
            (b">>=", TokenKind::ShrEq),
            (b"...", TokenKind::Ellipsis),
            (b"..=", TokenKind::DotDotEq),
            (b"**", TokenKind::StarStar),
            (b"&&", TokenKind::AndAnd),
            (b"||", TokenKind::OrOr),
            (b"|>", TokenKind::PipeGt),
            (b"==", TokenKind::EqEq),
            (b"!=", TokenKind::NotEq),
            (b"<=", TokenKind::LtEq),
            (b">=", TokenKind::GtEq),
            (b":=", TokenKind::ColonEq),
            (b"->", TokenKind::Arrow),
            (b"=>", TokenKind::FatArrow),
            (b"::", TokenKind::ColonColon),
            (b"+=", TokenKind::PlusEq),
            (b"-=", TokenKind::MinusEq),
            (b"*=", TokenKind::StarEq),
            (b"/=", TokenKind::SlashEq),
            (b"%=", TokenKind::PercentEq),
            (b"&=", TokenKind::AmpEq),
            (b"|=", TokenKind::PipeEq),
            (b"^=", TokenKind::CaretEq),
            (b"<<", TokenKind::Shl),
            (b">>", TokenKind::Shr),
            (b"..", TokenKind::DotDot),
        ];
        if let Some(&(spelling, kind)) = candidates
            .iter()
            .find(|(spelling, _)| self.bytes[self.pos..].starts_with(spelling))
        {
            self.pos += spelling.len();
            self.tokens.push(kind, start, self.pos, self.line);
            return;
        }
        let kind = match self.bytes[self.pos] {
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b'[' => TokenKind::LBracket,
            b']' => TokenKind::RBracket,
            b'{' => TokenKind::LBrace,
            b'}' => TokenKind::RBrace,
            b',' => TokenKind::Comma,
            b'.' => TokenKind::Dot,
            b':' => TokenKind::Colon,
            b';' => TokenKind::Semicolon,
            b'+' => TokenKind::Plus,
            b'-' => TokenKind::Minus,
            b'*' => TokenKind::Star,
            b'/' => TokenKind::Slash,
            b'%' => TokenKind::Percent,
            b'&' => TokenKind::Amp,
            b'|' => TokenKind::Pipe,
            b'^' => TokenKind::Caret,
            b'~' => TokenKind::Tilde,
            b'=' => TokenKind::Eq,
            b'<' => TokenKind::Lt,
            b'>' => TokenKind::Gt,
            b'?' => TokenKind::Question,
            b'!' => TokenKind::Bang,
            b'$' => TokenKind::Dollar,
            b'@' => TokenKind::At,
            _ => TokenKind::Error,
        };
        self.pos += char_len(self.source, self.pos);
        if kind == TokenKind::Error {
            self.error(
                if self.source[start..self.pos].starts_with('\u{feff}') {
                    Code::UnexpectedBom
                } else {
                    Code::InvalidToken
                },
                start,
                self.pos,
            );
        } else if kind == TokenKind::Semicolon {
            self.error(Code::ReservedSemicolon, start, self.pos);
        }
        self.tokens.push(kind, start, self.pos, self.line);
    }

    fn peek_byte(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }

    fn error(&mut self, code: Code, start: usize, end: usize) {
        self.diagnostics.push(diagnostic(code, start, end));
    }
}

fn build_lines(source: &[u8], tokens: &mut TokenBuf) {
    let mut start = 0;
    loop {
        let mut end = start;
        while end < source.len() && !matches!(source[end], b'\n' | b'\r') {
            end += 1;
        }
        let mut indent = 0_usize;
        while start + indent < end && source[start + indent] == b' ' {
            indent += 1;
        }
        let slice = &source[start..end];
        let mut flags = 0;
        if slice.iter().any(|byte| !byte.is_ascii()) {
            flags |= LineFlags::NON_ASCII;
        }
        if slice.contains(&b'\t') {
            flags |= LineFlags::HAS_TAB;
        }
        tokens.line_start.push(to_u32(start));
        tokens.line_tok.push(TokenIdx::from_raw(NONE));
        tokens
            .line_indent
            .push(indent.try_into().unwrap_or(u16::MAX));
        tokens.line_flags.push(LineFlags(flags));
        if end == source.len() {
            break;
        }
        start = end + usize::from(source[end] == b'\r' && source.get(end + 1) == Some(&b'\n')) + 1;
        if start > source.len() {
            break;
        }
    }
}

fn line_is_blank(source: &[u8], line: usize, starts: &[u32]) -> bool {
    let start = starts[line] as usize;
    let end = starts
        .get(line + 1)
        .map_or(source.len(), |value| *value as usize);
    source[start..end].iter().all(u8::is_ascii_whitespace)
}

fn keyword(text: &str) -> Option<TokenKind> {
    Some(match text {
        "Self" => TokenKind::KwSelfType,
        "break" => TokenKind::KwBreak,
        "continue" => TokenKind::KwContinue,
        "data" => TokenKind::KwData,
        "defer" => TokenKind::KwDefer,
        "dyn" => TokenKind::KwDyn,
        "else" => TokenKind::KwElse,
        "enum" => TokenKind::KwEnum,
        "false" => TokenKind::KwFalse,
        "fn" => TokenKind::KwFn,
        "for" => TokenKind::KwFor,
        "if" => TokenKind::KwIf,
        "impl" => TokenKind::KwImpl,
        "in" => TokenKind::KwIn,
        "is" => TokenKind::KwIs,
        "let" => TokenKind::KwLet,
        "match" => TokenKind::KwMatch,
        "mut" => TokenKind::KwMut,
        "pass" => TokenKind::KwPass,
        "pub" => TokenKind::KwPub,
        "return" => TokenKind::KwReturn,
        "self" => TokenKind::KwSelfValue,
        "tests" => TokenKind::KwTests,
        "trait" => TokenKind::KwTrait,
        "true" => TokenKind::KwTrue,
        "type" => TokenKind::KwType,
        "while" => TokenKind::KwWhile,
        _ => return None,
    })
}

fn is_ident_start_byte(byte: u8, source: &str, pos: usize) -> bool {
    byte == b'_'
        || byte.is_ascii_alphabetic()
        || (!byte.is_ascii() && source[pos..].chars().next().is_some_and(is_xid_start))
}

fn is_nfc(text: &str) -> bool {
    text.nfc().eq(text.chars())
}

fn digit_for_radix(byte: u8, radix: u32) -> bool {
    (byte as char).is_digit(radix)
}

fn valid_escape(source: &[u8], pos: usize) -> bool {
    match source.get(pos + 1).copied() {
        Some(b'\\' | b'"' | b'\'' | b'n' | b'r' | b't' | b'0' | b'$') => true,
        Some(b'u') => {
            let mut at = pos + 2;
            if source.get(at) != Some(&b'{') {
                return false;
            }
            at += 1;
            let first = at;
            while source.get(at).is_some_and(u8::is_ascii_hexdigit) && at - first < 6 {
                at += 1;
            }
            at > first && source.get(at) == Some(&b'}')
        }
        _ => false,
    }
}

fn char_len(source: &str, pos: usize) -> usize {
    source[pos..].chars().next().map_or(1, char::len_utf8)
}

fn diagnostic(code: Code, start: usize, end: usize) -> Diagnostic {
    Diagnostic::error(
        code,
        Span {
            file: FileId::from_raw(0),
            lo: to_u32(start),
            hi: to_u32(end),
        },
    )
}

fn to_u32(value: usize) -> u32 {
    value.try_into().unwrap_or(u32::MAX - 1)
}

#[cfg(test)]
mod tests {
    use super::{CommentKind, TokenKind, lex};

    #[test]
    fn lexes_losslessly() {
        let source = "## docs\nfn café(x: i32) -> string:\n    r\"hello $x\" # tail\n";
        let lexed = lex(source.as_bytes());
        assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
        assert_eq!(lexed.tokens.reconstruct(source), source);
        assert_eq!(
            lexed.tokens.com_kind,
            [CommentKind::Doc, CommentKind::Plain]
        );
        assert!(lexed.tokens.kind.contains(&TokenKind::KwFn));
    }

    #[test]
    fn recognizes_longest_operators_and_dyn() {
        let source = "dyn Show ...= ..= >>= **=";
        let kinds = lex(source.as_bytes()).tokens.kind;
        assert_eq!(
            kinds,
            [
                TokenKind::KwDyn,
                TokenKind::Ident,
                TokenKind::EllipsisEq,
                TokenKind::DotDotEq,
                TokenKind::ShrEq,
                TokenKind::StarStar,
                TokenKind::Eq
            ]
        );
    }
}
