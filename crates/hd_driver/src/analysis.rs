//! Optional post-verification analysis: the hook a lint run attaches and
//! its per-module findings cache. The walk and every rule live in the
//! consumer; a plain check or build attaches none and does no analysis work.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use hd_base::{FileId, Hash128, NodeIdx, Span};
use hd_cache::EntryKind;
use hd_check::body::BodyCx;
use hd_check::comparison::SourceFacts;
use hd_diag::{Code, DiagBuf, FixSafety, Severity};
use hd_project::MainSite;
use hd_resolve::Src;
use hd_resolve::facts::DeclView;

use crate::{Lines, Reader, Run, Writer, u32_of};

/// What an analysis consumer asks a run to collect beyond a plain check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Requests(u8);

impl Requests {
    pub const NONE: Self = Self(0);
    /// Call [`AnalysisSink::module`] for each verified root module.
    pub const MODULE: Self = Self(1 << 0);
    /// Record checked body facts, and call the module hook.
    pub const CHECKED: Self = Self(1 << 1);
    /// Give the module hook the module's declarations, and call it.
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
    const fn per_module(self) -> bool {
        self.0 & Self::PER_MODULE.0 != 0
    }
}

/// One verified root module with the facts its run collected.
#[derive(Clone, Copy)]
pub struct Module<'v> {
    pub src: Src<'v>,
    pub path: &'v str,
    /// The checked body facts, when the consumer asked for them.
    pub facts: Option<&'v SourceFacts>,
    /// The module's scope, items and derivation blocks, for the written
    /// names and decorators the derivation lints read, when the consumer
    /// asked for them.
    pub decls: Option<DeclView<'v>>,
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

/// The consumer a run attaches.
pub(crate) type Sink<'a> = &'a dyn AnalysisSink;

/// The attached consumer, what it asked for, and each module's findings.
pub(crate) struct Lint<'a> {
    sink: Sink<'a>,
    requests: Requests,
    findings: Vec<OnceLock<DiagBuf>>,
}

impl<'a> Lint<'a> {
    /// The run's lint state for `modules` modules; none when no consumer is
    /// attached or it asks for nothing.
    pub(crate) fn new(sink: Option<Sink<'a>>, modules: usize) -> Option<Self> {
        let sink = sink?;
        let requests = sink.requests();
        (requests != Requests::NONE).then(|| Self {
            sink,
            requests,
            findings: (0..modules).map(|_| OnceLock::new()).collect(),
        })
    }
}

/// The run's analysis hooks. A plain check or build attaches no consumer,
/// so each is one `None` test.
impl Run<'_> {
    /// The lint state, when `m` is a root module and the consumer asked for
    /// all of `requests`.
    fn lint_on(&self, m: usize, requests: Requests) -> Option<&Lint<'_>> {
        self.lint
            .as_ref()
            .filter(|lint| self.table.modules[m].package == 0 && lint.requests.contains(requests))
    }

    /// Whether root module `m`'s check records the checked facts.
    pub(crate) fn collect_checked_facts(&self, m: usize) -> bool {
        self.lint_on(m, Requests::CHECKED).is_some()
    }

    /// Whether the module hook runs on root module `m`.
    pub(crate) fn lint_runs(&self, m: usize) -> bool {
        self.lint_on(m, Requests::NONE)
            .is_some_and(|lint| lint.requests.per_module())
    }

    /// Root module `m`'s cached findings, when the hook runs on it. Typed
    /// facts are not persisted in Check entries, so a miss rechecks the
    /// root module's bodies; dependencies keep their Check hits.
    pub(crate) fn lint_cached(&self, m: usize, key: Hash128) -> Option<DiagBuf> {
        let lint = self.lint.as_ref().filter(|_| self.lint_runs(m))?;
        let sections = self.lookup(EntryKind::Lint, key)?;
        let [bytes] = sections.as_slice() else {
            return None;
        };
        decode_cache(
            bytes.as_ref(),
            &self.lines(m),
            FileId::from_raw(u32_of(m)),
            lint.sink,
        )
    }

    /// Keeps root module `m`'s findings from its cache entry.
    pub(crate) fn lint_keep(&self, m: usize, findings: DiagBuf) {
        if let Some(lint) = &self.lint {
            let _ = lint.findings[m].set(findings);
        }
    }

    /// Runs the module hook on root module `m`, checked without errors.
    pub(crate) fn lint_module(&self, m: usize, cx: &BodyCx<'_>, blocks: &[NodeIdx]) {
        let Some(lint) = self.lint.as_ref().filter(|_| self.lint_runs(m)) else {
            return;
        };
        let mut findings = DiagBuf::default();
        let facts = cx.source_facts.as_ref().map(std::cell::RefCell::borrow);
        let derives = self.lint_on(m, Requests::DERIVATIONS).is_some();
        lint.sink.module(
            &Module {
                src: cx.src,
                path: &self.table.modules[m].path,
                facts: facts.as_deref(),
                decls: derives.then(|| {
                    let folder = self.table.modules[m].folder;
                    DeclView::new(cx.names, folder, cx.scope, cx.lookup, blocks)
                }),
                main_site: self.main_site(m),
            },
            &mut findings,
        );
        let _ = lint.findings[m].set(findings);
    }

    /// Root module `m`'s `main` site, when the consumer asked for it.
    fn main_site(&self, m: usize) -> Option<MainSite> {
        self.lint_on(m, Requests::MAIN_SITE)?;
        let module = &self.table.modules[m];
        let skim = &self.skim_of(m).file;
        let (lo, hi, public) = skim
            .public_main
            .map(|(lo, hi)| (lo, hi, true))
            .or_else(|| skim.private_main.map(|(lo, hi)| (lo, hi, false)))?;
        Some(MainSite {
            span: Span {
                file: module.file,
                lo,
                hi,
            },
            public,
            entry_module: self.entry_module(m),
            make_public_exact: !self
                .table
                .modules
                .iter()
                .enumerate()
                .any(|(other, candidate)| {
                    candidate.package == 0 && self.skim_of(other).file.public_main.is_some()
                }),
            role: module.role,
            entry: module.entry,
            source_root: self.table.files[module.file.idx()].starts_with("src/"),
        })
    }

    /// Stores root module `m`'s findings beside its Check entry `key`.
    pub(crate) fn lint_store(&self, m: usize, key: Hash128, lines: &Lines<'_>) {
        let Some(lint) = self.lint_on(m, Requests::NONE) else {
            return;
        };
        let empty = DiagBuf::default();
        let findings = lint.findings[m].get().unwrap_or(&empty);
        if let Some(bytes) = encode_cache(findings, lines, FileId::from_raw(u32_of(m))) {
            self.put(EntryKind::Lint, key, &[&bytes]);
        }
    }

    /// Sends each fetched package's first cached `dbg` reference to the
    /// consumer. Dependency bodies stay on ordinary Check cache hits; the
    /// reference comes from the cached section.
    pub(crate) fn lint_dbg_in_dependencies(&self) {
        let Some(lint) = self
            .lint
            .as_ref()
            .filter(|lint| lint.requests.contains(Requests::FETCHED_DEBUG))
        else {
            return;
        };
        let mut warned = BTreeSet::new();
        for (m, module) in self.table.modules.iter().enumerate() {
            let package = usize::from(module.package);
            if !self.fetched.get(package).copied().unwrap_or(false) {
                continue;
            }
            let Some(reference) = self.dbg_refs[m].get().and_then(|refs| refs.first()) else {
                continue;
            };
            if warned.insert(package) {
                let mut findings = DiagBuf::default();
                lint.sink.fetched_debug(
                    &self.table.packages[package],
                    reference.span,
                    self.src(m),
                    &mut findings,
                );
                let _ = lint.findings[m].set(findings);
            }
        }
    }

    /// Every module's findings, in source table order.
    pub(crate) fn lint_findings(&self) -> DiagBuf {
        let mut all = DiagBuf::default();
        for findings in self.lint.iter().flat_map(|lint| &lint.findings) {
            if let Some(findings) = findings.get() {
                all.append(findings);
            }
        }
        all
    }
}

/// Encodes one module's policy-free findings with line-relative spans.
/// Root keys are checker-local identities and cannot cross a cache boundary.
fn encode_cache(findings: &DiagBuf, lines: &Lines<'_>, file: FileId) -> Option<Vec<u8>> {
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
fn decode_cache(
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
