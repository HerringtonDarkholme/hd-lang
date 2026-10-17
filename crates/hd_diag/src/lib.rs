#![forbid(unsafe_code)]

use hd_base::Span;

pub mod buf;
pub use buf::{CauseKind, DiagBuf, DiagMark, FixEdit, FixSafety, INTERNAL_ERROR, RootKey};

mod codes;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub primary: Span,
    /// What the user reads: the situation, not the code.
    pub message: String,
}

impl Diagnostic {
    #[must_use]
    pub fn error(code: Code, primary: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            primary,
            message: message.into(),
        }
    }
}
