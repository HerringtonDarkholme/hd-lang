//! Diagnostic records as columns (data-structures.md §3.8) and the compact
//! and JSON renderers (checking-and-tir.md §4.14).

use std::fmt::Write as _;

use hd_base::{DefId, Range32, Span};

use crate::{Code, Severity};

/// Exact passed the re-parse check, so `hd fix` applies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixSafety {
    Exact,
    Suggestion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CauseKind {
    Type,
    Name,
    Bound,
    Row,
    Init,
    Syntax,
}

/// One diagnostic per root cause (§4.14).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RootKey {
    pub item: DefId,
    pub cause: CauseKind,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixEdit {
    pub span: Span,
    pub text: Range32,
}

/// One row per diagnostic, plus pooled parts. Messages are kept as text
/// here; the design's `MsgId` templates come with the generated code list.
#[derive(Clone, Debug, Default)]
pub struct DiagBuf {
    pub code: Vec<Code>,
    pub severity: Vec<Severity>,
    pub primary: Vec<Span>,
    pub message: Vec<Range32>,
    pub labels: Vec<Range32>,
    pub fixes: Vec<Range32>,
    pub root: Vec<Option<RootKey>>,
    pub label_span: Vec<Span>,
    pub label_msg: Vec<Range32>,
    pub fix_title: Vec<Range32>,
    pub fix_safety: Vec<FixSafety>,
    pub fix_edits: Vec<Range32>,
    pub edits: Vec<FixEdit>,
    pub text: String,
}

impl DiagBuf {
    fn text(&mut self, s: &str) -> Range32 {
        let start = u32::try_from(self.text.len()).expect("diag text");
        self.text.push_str(s);
        Range32::new(start, u32::try_from(s.len()).expect("diag text"))
    }

    #[must_use]
    pub fn get_text(&self, r: Range32) -> &str {
        &self.text[r.range()]
    }

    /// Pushes one diagnostic; a second one with the same root is dropped.
    pub fn push(
        &mut self,
        code: Code,
        severity: Severity,
        primary: Span,
        message: &str,
        root: Option<RootKey>,
    ) -> bool {
        if root.is_some() && self.root.contains(&root) {
            return false;
        }
        let message = self.text(message);
        self.code.push(code);
        self.severity.push(severity);
        self.primary.push(primary);
        self.message.push(message);
        let nl = u32::try_from(self.label_span.len()).expect("labels");
        self.labels.push(Range32::new(nl, 0));
        let nf = u32::try_from(self.fix_title.len()).expect("fixes");
        self.fixes.push(Range32::new(nf, 0));
        self.root.push(root);
        true
    }

    /// Error-severity diagnostic with no root key.
    pub fn error(&mut self, code: Code, primary: Span, message: &str) {
        self.push(code, Severity::Error, primary, message, None);
    }

    /// Appends another buffer's diagnostics (messages only; this run's
    /// stages attach no labels or fixes after parsing).
    pub fn append(&mut self, other: &DiagBuf) {
        for i in 0..other.len() {
            self.push(
                other.code[i],
                other.severity[i],
                other.primary[i],
                other.get_text(other.message[i]),
                other.root[i],
            );
        }
    }

    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.severity.contains(&Severity::Error)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.code.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }

    /// Content order (scheduler.md §6.5): by file, position, then code.
    #[must_use]
    pub fn content_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.len()).collect();
        order.sort_by_key(|&i| {
            (
                self.primary[i].file.raw(),
                self.primary[i].lo,
                self.code[i].as_str(),
            )
        });
        order
    }

    /// Compact form: `severity: location: code: message`, in content order.
    /// `location` is the caller's `file:line:column` (or byte range). The
    /// message's leading code, if it repeats the code, is dropped so the
    /// code prints once.
    #[must_use]
    pub fn render_compact(&self, location: &dyn Fn(Span) -> String) -> String {
        let mut out = String::new();
        for i in self.content_order() {
            let p = self.primary[i];
            let sev = match self.severity[i] {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            let code = self.code[i].as_str();
            let text = self.get_text(self.message[i]);
            let message = text
                .strip_prefix(code)
                .and_then(|rest| rest.strip_prefix(": "))
                .unwrap_or(text);
            let _ = writeln!(out, "{sev}: {}: {code}: {message}", location(p));
        }
        out
    }

    /// JSON form: one object per diagnostic, in content order.
    #[must_use]
    pub fn render_json(&self) -> String {
        let mut out = String::from("[");
        for (n, i) in self.content_order().into_iter().enumerate() {
            if n > 0 {
                out.push(',');
            }
            let p = self.primary[i];
            let _ = write!(
                out,
                "{{\"code\":\"{}\",\"file\":{},\"lo\":{},\"hi\":{},\"message\":\"{}\"}}",
                self.code[i].as_str(),
                p.file.raw(),
                p.lo,
                p.hi,
                self.get_text(self.message[i])
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
            );
        }
        out.push(']');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{CauseKind, DiagBuf, RootKey};
    use crate::{Code, Severity};
    use hd_base::{DefId, FileId, Span};

    #[test]
    fn one_diagnostic_per_root_and_content_order() {
        let mut b = DiagBuf::default();
        let root = Some(RootKey {
            item: DefId::from_raw(1),
            cause: CauseKind::Type,
            value: 0,
        });
        let s = |lo| Span {
            file: FileId::from_raw(0),
            lo,
            hi: lo + 1,
        };
        assert!(b.push(Code::SyntaxError, Severity::Error, s(9), "late", None));
        assert!(b.push(Code::SyntaxError, Severity::Error, s(2), "early", root));
        assert!(!b.push(Code::SyntaxError, Severity::Error, s(3), "dup", root));
        let text = b.render_compact(&|_| "a.hd:1:1".into());
        assert!(text.find("early").expect("early") < text.find("late").expect("late"));
        assert!(b.render_json().starts_with("[{\"code\":\"syntax-error\""));
    }
}
