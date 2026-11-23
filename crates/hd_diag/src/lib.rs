#![forbid(unsafe_code)]

use hd_base::Span;

pub mod buf;
pub use buf::{CauseKind, DiagBuf, DiagMark, FixEdit, FixSafety, INTERNAL_ERROR, RootKey};

mod codes;
pub mod suggest;
pub use codes::{Code, Phase};

impl Code {
    /// A lexical or grammar diagnostic, reported by `hd_syntax`.
    #[must_use]
    pub const fn is_syntax(self) -> bool {
        matches!(self.phase(), Phase::Parse)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// An owned fix before diagnostics enter the pooled `DiagBuf` columns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticFix {
    pub message: String,
    pub safety: FixSafety,
    pub edits: Vec<(Span, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub primary: Span,
    /// What the user reads: the situation, not the code.
    pub message: String,
    pub fixes: Vec<DiagnosticFix>,
}

impl Diagnostic {
    #[must_use]
    pub fn error(code: Code, primary: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            primary,
            message: message.into(),
            fixes: Vec::new(),
        }
    }
}

/// A JSON string literal.
#[must_use]
pub fn quote(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
