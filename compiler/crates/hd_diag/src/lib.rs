#![forbid(unsafe_code)]

use hd_base::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Code {
    InvalidToken,
    UnexpectedBom,
    TabWhitespace,
    InvalidEscape,
    UnterminatedString,
    UnmatchedDelimiter,
    UnclosedDelimiter,
    InvalidDedent,
    UnexpectedIndentation,
    ReservedSemicolon,
    SyntaxError,
}

impl Code {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidToken => "invalid-token",
            Self::UnexpectedBom => "unexpected-bom",
            Self::TabWhitespace => "tab-whitespace",
            Self::InvalidEscape => "invalid-escape",
            Self::UnterminatedString => "unterminated-string",
            Self::UnmatchedDelimiter => "unmatched-delimiter",
            Self::UnclosedDelimiter => "unclosed-delimiter",
            Self::InvalidDedent => "invalid-dedent",
            Self::UnexpectedIndentation => "unexpected-indentation",
            Self::ReservedSemicolon => "reserved-semicolon",
            Self::SyntaxError => "syntax-error",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub primary: Span,
}

impl Diagnostic {
    #[must_use]
    pub const fn error(code: Code, primary: Span) -> Self {
        Self {
            code,
            severity: Severity::Error,
            primary,
        }
    }
}
