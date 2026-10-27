//! Optional post-verification analysis dispatch.

use hd_base::SliceSort as _;
use hd_base::{FileId, Span, TokenIdx};
use hd_check::analysis::CheckedView;
use hd_diag::{Code, DiagBuf, FixSafety, Severity};
use hd_resolve::Src;
use hd_syntax::{Decl, SyntaxKind, TokenKind};

use crate::{Lines, Reader, Writer};

/// Source event families requested by an analysis consumer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events(u8);

impl Events {
    pub const IDENT: Self = Self(1 << 0);
    pub const DECL: Self = Self(1 << 1);
    pub const MODULE: Self = Self(1 << 2);
    pub const ALL: Self = Self(Self::IDENT.0 | Self::DECL.0 | Self::MODULE.0);

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// A declaration after its complete source subtree has been visited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclEvent {
    pub kind: SyntaxKind,
    pub span: Span,
    pub name: Option<Span>,
    pub public: bool,
}

/// A module after all of its source and declaration events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleEvent<'v> {
    pub path: &'v str,
    pub span: Span,
}

/// One neutral event in the verified module's source-order walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event<'v> {
    Ident(Span),
    Decl(DeclEvent),
    Module(ModuleEvent<'v>),
}

impl Event<'_> {
    #[must_use]
    pub const fn family(self) -> Events {
        match self {
            Self::Ident(_) => Events::IDENT,
            Self::Decl(_) => Events::DECL,
            Self::Module(_) => Events::MODULE,
        }
    }
}

/// A read-only consumer of one verified, source-order module walk.
pub trait AnalysisSink: Sync {
    fn interests(&self) -> Events;
    /// Resolves a stable diagnostic name from a policy-free cache entry.
    fn diagnostic_code(&self, name: &str) -> Option<Code>;
    fn event(&self, event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf);
}

/// Encodes one module's policy-free findings with line-relative spans.
/// Root keys are checker-local identities and cannot cross a cache boundary.
pub(crate) fn encode_cache(findings: &DiagBuf, lines: &Lines<'_>, file: FileId) -> Option<Vec<u8>> {
    let mut w = Writer::default();
    w.len_of(&findings.code);
    for i in 0..findings.len() {
        if findings.root[i].is_some() || findings.primary[i].file != file {
            return None;
        }
        w.str(findings.code[i].as_str());
        w.u8(match findings.severity[i] {
            Severity::Error => 0,
            Severity::Warning => 1,
        });
        lines.write(&mut w, findings.primary[i].lo);
        lines.write(&mut w, findings.primary[i].hi);
        w.str(findings.get_text(findings.message[i]));
        let labels = findings.labels[i];
        w.u32(labels.len);
        for label in &findings.label_span[labels.range()] {
            if label.file != file {
                return None;
            }
        }
        for label in labels.start..labels.start + labels.len {
            let label = label as usize;
            lines.write(&mut w, findings.label_span[label].lo);
            lines.write(&mut w, findings.label_span[label].hi);
            w.str(findings.get_text(findings.label_msg[label]));
        }
        let fixes = findings.fixes[i];
        w.u32(fixes.len);
        for fix in fixes.start..fixes.start + fixes.len {
            let fix = fix as usize;
            w.str(findings.get_text(findings.fix_title[fix]));
            w.u8(match findings.fix_safety[fix] {
                FixSafety::Exact => 0,
                FixSafety::Suggestion => 1,
            });
            let edits = findings.fix_edits[fix];
            w.u32(edits.len);
            for edit in &findings.edits[edits.range()] {
                if edit.span.file != file {
                    return None;
                }
                lines.write(&mut w, edit.span.lo);
                lines.write(&mut w, edit.span.hi);
                w.str(findings.get_text(edit.text));
            }
        }
    }
    Some(w.bytes)
}

/// Decodes [`encode_cache`] against the current source line starts.
pub(crate) fn decode_cache(
    bytes: &[u8],
    lines: &Lines<'_>,
    file: FileId,
    sink: &dyn AnalysisSink,
) -> Option<DiagBuf> {
    let mut r = Reader::new(bytes);
    let mut findings = DiagBuf::default();
    for _ in 0..r.count() {
        let code = sink.diagnostic_code(r.str())?;
        let severity = match r.u8() {
            0 => Severity::Error,
            1 => Severity::Warning,
            _ => return None,
        };
        let primary = Span {
            file,
            lo: lines.read(&mut r),
            hi: lines.read(&mut r),
        };
        let message = r.str().to_owned();
        findings.push(code, severity, primary, &message, None);
        for _ in 0..r.count() {
            let span = Span {
                file,
                lo: lines.read(&mut r),
                hi: lines.read(&mut r),
            };
            let message = r.str().to_owned();
            findings.add_label(span, &message);
        }
        for _ in 0..r.count() {
            let title = r.str().to_owned();
            let safety = match r.u8() {
                0 => FixSafety::Exact,
                1 => FixSafety::Suggestion,
                _ => return None,
            };
            let edits: Vec<(Span, String)> = (0..r.count())
                .map(|_| {
                    let span = Span {
                        file,
                        lo: lines.read(&mut r),
                        hi: lines.read(&mut r),
                    };
                    (span, r.str().to_owned())
                })
                .collect();
            let edits: Vec<(Span, &str)> = edits
                .iter()
                .map(|(span, text)| (*span, text.as_str()))
                .collect();
            findings.add_fix(&title, safety, &edits);
        }
    }
    r.ok().then_some(findings)
}

enum Pending {
    Ident(Span),
    Decl(DeclEvent),
}

impl Pending {
    const fn key(&self) -> (u32, u8) {
        match self {
            Self::Ident(span) => (span.lo, 0),
            // Declaration events are finish events, after everything whose
            // source starts before the declaration's end.
            Self::Decl(event) => (event.span.hi, 1),
        }
    }
}

pub(crate) fn walk(
    sink: &dyn AnalysisSink,
    interests: Events,
    src: Src<'_>,
    module_path: &str,
    out: &mut DiagBuf,
) {
    let view = CheckedView::from_source(src);
    let mut pending = Vec::new();
    if interests.contains(Events::IDENT) {
        for raw in 0..src.parse.tokens.len() {
            let token = TokenIdx::from_raw(u32::try_from(raw).expect("source tokens"));
            if src.parse.tokens.kind(token) == TokenKind::Ident {
                pending.push(Pending::Ident(src.token_span(token)));
            }
        }
    }
    if interests.contains(Events::DECL) {
        for node in src.root().descendants() {
            let Some(decl) = Decl::cast(node) else {
                continue;
            };
            pending.push(Pending::Decl(DeclEvent {
                kind: node.kind(),
                span: src.span(node),
                name: decl
                    .name(&src.parse.tokens)
                    .map(|name| src.token_span(name)),
                public: decl.is_pub(&src.parse.tokens),
            }));
        }
    }
    pending.sort_stable_by_key(Pending::key);
    for event in pending {
        let event = match event {
            Pending::Ident(span) => Event::Ident(span),
            Pending::Decl(event) => Event::Decl(event),
        };
        sink.event(&event, &view, out);
    }
    if interests.contains(Events::MODULE) {
        sink.event(
            &Event::Module(ModuleEvent {
                path: module_path,
                span: src.span(src.root()),
            }),
            &view,
            out,
        );
    }
}
