//! Diagnostic records as columns (`docs/compiler/data-structures.md` §3.8) and the compact
//! and JSON renderers (`crates/hd_tir/DESIGN.md` §4.14).

use hd_base::SliceSort as _;
use std::fmt::Write as _;

use hd_base::{DefId, Range32, Span};

use crate::{Code, Severity};

/// What an internal error's message starts with
/// ([`DiagBuf::internal_error`]). It names no diagnostic code: the
/// specification's table has none for a compiler bug.
pub const INTERNAL_ERROR: &str = "internal-error: ";

/// Exact is safe to apply, so `hd fix` tries it; `hd fix` keeps it only if
/// the check after it adds no diagnostic (`crates/hd_cli/src/fix_cmd.rs`).
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

/// A `DiagBuf` rollback point: its columns' lengths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagMark {
    rows: u32,
    label_span: u32,
    fix_title: u32,
    edits: u32,
    text: u32,
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

    /// Attaches a fix-it to the diagnostic pushed last: its title and the
    /// edits that apply it, each a span and the text that replaces it.
    pub fn add_fix(&mut self, title: &str, safety: FixSafety, edits: &[(Span, &str)]) {
        let Some(row) = self.fixes.last_mut() else {
            return;
        };
        row.len += 1;
        let title = self.text(title);
        let start = u32::try_from(self.edits.len()).expect("edits");
        self.fix_title.push(title);
        self.fix_safety.push(safety);
        self.fix_edits.push(Range32::new(
            start,
            u32::try_from(edits.len()).expect("edits"),
        ));
        for (span, text) in edits {
            let text = self.text(text);
            self.edits.push(FixEdit { span: *span, text });
        }
    }

    /// Attaches a secondary span and message to the diagnostic pushed last.
    pub fn add_label(&mut self, span: Span, message: &str) {
        let Some(row) = self.labels.last_mut() else {
            return;
        };
        row.len += 1;
        let message = self.text(message);
        self.label_span.push(span);
        self.label_msg.push(message);
    }

    /// Error-severity diagnostic with no root key.
    pub fn error(&mut self, code: Code, primary: Span, message: &str) {
        self.push(code, Severity::Error, primary, message, None);
    }

    /// Reports a failed check of the compiler's own work, a compiler bug:
    /// an error with no diagnostic code, its message led by
    /// [`INTERNAL_ERROR`]. The row keeps the internal `unsupported` code,
    /// which marks a stage that stopped, so every count of stopped
    /// stages still sees it; it renders with no code ([`Self::shown_code`]).
    pub fn internal_error(&mut self, primary: Span, message: &str) {
        let text = format!("{INTERNAL_ERROR}{message}");
        self.push(Code::Unsupported, Severity::Error, primary, &text, None);
    }

    /// Whether row `i` is an internal error ([`Self::internal_error`]).
    #[must_use]
    pub fn is_internal(&self, i: usize) -> bool {
        self.code[i] == Code::Unsupported
            && self.get_text(self.message[i]).starts_with(INTERNAL_ERROR)
    }

    /// The code row `i` shows: its own, or none for an internal error.
    #[must_use]
    pub fn shown_code(&self, i: usize) -> Option<Code> {
        (!self.is_internal(i)).then_some(self.code[i])
    }

    /// Appends another buffer's diagnostics, labels and fix-its included.
    pub fn append(&mut self, other: &DiagBuf) {
        self.append_if(other, &|_| true);
    }

    /// [`Self::append`] of only the diagnostics whose code `keep` takes,
    /// in their order.
    pub fn append_if(&mut self, other: &DiagBuf, keep: &dyn Fn(Code) -> bool) {
        for i in 0..other.len() {
            if !keep(other.code[i]) {
                continue;
            }
            if !self.push(
                other.code[i],
                other.severity[i],
                other.primary[i],
                other.get_text(other.message[i]),
                other.root[i],
            ) {
                continue;
            }
            let labels = other.labels[i];
            for label in labels.start..labels.start + labels.len {
                let label = label as usize;
                self.add_label(
                    other.label_span[label],
                    other.get_text(other.label_msg[label]),
                );
            }
            let fixes = other.fixes[i];
            for f in fixes.start..fixes.start + fixes.len {
                let f = f as usize;
                let edits: Vec<(Span, &str)> = other.edits[other.fix_edits[f].range()]
                    .iter()
                    .map(|e| (e.span, other.get_text(e.text)))
                    .collect();
                self.add_fix(
                    other.get_text(other.fix_title[f]),
                    other.fix_safety[f],
                    &edits,
                );
            }
        }
    }

    /// A rollback point: every column's length (`crates/hd_check/DESIGN.md` (type checking) §3.5).
    #[must_use]
    pub fn mark(&self) -> DiagMark {
        let n = |v: usize| u32::try_from(v).expect("diags");
        DiagMark {
            rows: n(self.code.len()),
            label_span: n(self.label_span.len()),
            fix_title: n(self.fix_title.len()),
            edits: n(self.edits.len()),
            text: n(self.text.len()),
        }
    }

    /// Drops every diagnostic pushed since `m`.
    pub fn truncate(&mut self, m: DiagMark) {
        let rows = m.rows as usize;
        self.code.truncate(rows);
        self.severity.truncate(rows);
        self.primary.truncate(rows);
        self.message.truncate(rows);
        self.labels.truncate(rows);
        self.fixes.truncate(rows);
        self.root.truncate(rows);
        self.label_span.truncate(m.label_span as usize);
        self.label_msg.truncate(m.label_span as usize);
        self.fix_title.truncate(m.fix_title as usize);
        self.fix_safety.truncate(m.fix_title as usize);
        self.fix_edits.truncate(m.fix_title as usize);
        self.edits.truncate(m.edits as usize);
        self.text.truncate(m.text as usize);
    }

    /// The error-severity diagnostics pushed since `m`.
    #[must_use]
    pub fn errors_since(&self, m: DiagMark) -> usize {
        self.severity
            .get(m.rows as usize..)
            .map_or(0, |s| s.iter().filter(|x| **x == Severity::Error).count())
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

    /// Content order (`crates/hd_sched/DESIGN.md` §6.5): by file, position, then code.
    #[must_use]
    pub fn content_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.len()).collect();
        order.sort_stable_by_key(|&i| {
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
            let text = self.get_text(self.message[i]);
            let Some(code) = self.shown_code(i).map(Code::as_str) else {
                let _ = writeln!(out, "{sev}: {}: {text}", location(p));
                continue;
            };
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
                "{{\"code\":{},\"file\":{},\"lo\":{},\"hi\":{},\"message\":\"{}\"}}",
                self.shown_code(i)
                    .map_or_else(|| "null".to_owned(), |c| format!("\"{}\"", c.as_str())),
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

    #[test]
    fn an_internal_error_renders_with_no_code() {
        let mut b = DiagBuf::default();
        let at = Span {
            file: FileId::from_raw(0),
            lo: 0,
            hi: 0,
        };
        b.internal_error(
            at,
            "the compiler hit a bug in stage Body (x); please report it",
        );
        b.error(Code::Unsupported, at, "stage Body: y is not supported");
        assert!(b.is_internal(0) && !b.is_internal(1));
        assert_eq!(b.shown_code(0), None);
        let text = b.render_compact(&|_| "a.hd:1:1".into());
        assert!(
            text.contains("error: a.hd:1:1: internal-error: the compiler hit a bug"),
            "{text}"
        );
        assert!(
            text.contains("a.hd:1:1: unsupported: stage Body: y"),
            "{text}"
        );
        assert!(b.render_json().contains("{\"code\":null,"));
    }
}
