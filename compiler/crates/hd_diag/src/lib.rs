#![forbid(unsafe_code)]

use hd_base::Span;

pub mod buf;
pub use buf::{CauseKind, DiagBuf, FixEdit, FixSafety, RootKey};

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
