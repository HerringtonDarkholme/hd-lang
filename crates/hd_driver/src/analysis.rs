//! Optional post-verification analysis: the hook a lint run attaches and
//! its per-module findings cache. The walk and every rule live in the
//! consumer; a plain check or build attaches none and does no analysis work.

use hd_base::{FileId, Span};
use hd_check::analysis::CheckedView;
use hd_diag::{Code, DiagBuf, FixSafety, Severity};
use hd_project::MainSite;
use hd_resolve::Src;
use hd_resolve::facts::DerivationFacts;

use crate::{Lines, Reader, Writer};

/// What an analysis consumer asks a run to collect beyond a plain check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Requests(u8);

impl Requests {
    pub const NONE: Self = Self(0);
    /// Call [`AnalysisSink::module`] for each verified root module.
    pub const MODULE: Self = Self(1 << 0);
    /// Record checked body facts, and call the module hook.
    pub const CHECKED: Self = Self(1 << 1);
    /// Collect resolved derivation facts, and call the module hook.
    pub const DERIVATIONS: Self = Self(1 << 2);
    /// Find each root module's `main` site, and call the module hook.
    pub const MAIN_SITE: Self = Self(1 << 3);
    /// Call [`AnalysisSink::fetched_debug`] for fetched packages.
    pub const FETCHED_DEBUG: Self = Self(1 << 4);
    const PER_MODULE: Self =
        Self(Self::MODULE.0 | Self::CHECKED.0 | Self::DERIVATIONS.0 | Self::MAIN_SITE.0);

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the run calls the module hook.
    #[must_use]
    pub(crate) const fn per_module(self) -> bool {
        self.0 & Self::PER_MODULE.0 != 0
    }
}

/// One verified root module with the facts its run collected.
#[derive(Clone, Copy)]
pub struct Module<'v> {
    pub src: Src<'v>,
    pub path: &'v str,
    pub checked: CheckedView<'v>,
    pub derivations: Option<&'v DerivationFacts>,
    pub main_site: Option<MainSite>,
}

/// A read-only consumer of verified modules.
pub trait AnalysisSink: Sync {
    fn requests(&self) -> Requests;
    /// Resolves a stable diagnostic name from a policy-free cache entry.
    fn diagnostic_code(&self, name: &str) -> Option<Code>;
    /// One verified root module without errors.
    fn module(&self, module: &Module<'_>, out: &mut DiagBuf);
    /// The first cached `dbg` reference, at `span` in `src`, of one fetched
    /// package.
    fn fetched_debug(&self, package: &str, span: Span, src: Src<'_>, out: &mut DiagBuf);
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
