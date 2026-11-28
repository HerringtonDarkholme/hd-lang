//! Diagnostics as the CLI and the playground report them: one record per
//! diagnostic, whatever found it (a manifest rule, the command line, or the
//! compiler), written as text (`severity: file:line:column: code: message`)
//! or as a JSON line (`cli.json.diagnostic.*`, `cli.json.fix.*`). Messages
//! name test and task code by file (`module.test.integration.no-path`,
//! `cli.task.no-path`).

use std::fmt::Write as _;

use hd_base::{FileId, NotImplemented, Span, Stage};
use hd_diag::{Code, DiagBuf, Severity};

use crate::Output;

/// One edit of a fix-it: bytes `start..end` of `file` become `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub file: String,
    pub start: u32,
    pub end: u32,
    pub text: String,
}

/// One fix-it. `exact` is [`hd_diag::FixSafety::Exact`]: the compiler
/// offers it as the whole fix, so `hd check --fix` tries it
/// (`cli.check.fix.safe`); a
/// suggestion is only shown. JSON exposes uncertainty as `guess`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub message: String,
    pub edits: Vec<Edit>,
    pub exact: bool,
}

/// A secondary source span and its short label, separate from the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
}

/// One diagnostic. A command-line error has no code; a diagnostic that no
/// source text causes has no file (`cli.json.diagnostic.no-position`).
/// Lines and columns are 1-based; a column counts characters. The end is
/// where the primary span stops, for an editor to underline; the CLI's
/// text and JSON lines leave it out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    pub code: Option<Code>,
    pub severity: Severity,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub fixes: Vec<Fix>,
    pub labels: Vec<Label>,
}

impl Diag {
    #[must_use]
    pub fn error(code: Option<Code>, message: &str) -> Diag {
        Diag::new(code, Severity::Error, message)
    }

    #[must_use]
    pub fn new(code: Option<Code>, severity: Severity, message: &str) -> Diag {
        Diag {
            code,
            severity,
            message: message.to_owned(),
            file: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            fixes: Vec::new(),
            labels: Vec::new(),
        }
    }

    /// The same diagnostic at `file`, and at `line` when it has one.
    #[must_use]
    pub fn at(mut self, file: &str, line: Option<usize>) -> Diag {
        self.file = Some(file.to_owned());
        self.line = line;
        self.column = line.map(|_| 1);
        self
    }

    #[must_use]
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    #[must_use]
    pub fn severity_word(&self) -> &'static str {
        match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }

    /// The text line: `severity: file:line:column: code: message`, leaving
    /// out each part the diagnostic lacks, then one `    fix: message`
    /// line for each fix-it (`cli.text.fix-line`).
    #[must_use]
    pub fn text(&self) -> String {
        let mut s = format!("{}: ", self.severity_word());
        if let Some(file) = &self.file {
            s.push_str(file);
            if let Some(line) = self.line {
                let _ = write!(s, ":{line}:{}", self.column.unwrap_or(1));
            }
            s.push_str(": ");
        }
        if let Some(code) = self.code {
            let _ = write!(s, "{}: ", code.as_str());
        }
        s.push_str(&self.message);
        for label in &self.labels {
            let _ = write!(s, "\n    note: ");
            if let Some(file) = &label.file {
                s.push_str(file);
                if let Some(line) = label.line {
                    let _ = write!(s, ":{line}:{}", label.column.unwrap_or(1));
                }
                s.push_str(": ");
            }
            s.push_str(&label.message);
        }
        for f in &self.fixes {
            let _ = write!(s, "\n    fix: {}", f.message);
        }
        s
    }

    /// The JSON line (`cli.json.diagnostic.fields`, `cli.json.diagnostic.nonempty-fixes`).
    #[must_use]
    pub fn json(&self) -> String {
        let opt_num = |n: Option<usize>| n.map_or_else(|| "null".to_owned(), |n| n.to_string());
        let mut s = format!(
            "{{\"kind\":\"diagnostic\",\"code\":{},\"severity\":\"{}\",\"message\":{},\"file\":{},\"line\":{},\"column\":{}",
            self.code
                .map_or_else(|| "null".to_owned(), |c| quote(c.as_str())),
            self.severity_word(),
            quote(&self.message),
            self.file
                .as_deref()
                .map_or_else(|| "null".to_owned(), quote),
            opt_num(self.line),
            opt_num(self.column),
        );
        if !self.fixes.is_empty() {
            s.push_str(",\"fixes\":[");
            for (n, f) in self.fixes.iter().enumerate() {
                if n > 0 {
                    s.push(',');
                }
                let guess = !f.exact || self.fixes.len() > 1;
                let _ = write!(
                    s,
                    "{{\"message\":{},\"guess\":{guess},\"edits\":[",
                    quote(&f.message)
                );
                for (m, e) in f.edits.iter().enumerate() {
                    if m > 0 {
                        s.push(',');
                    }
                    let _ = write!(
                        s,
                        "{{\"file\":{},\"start\":{},\"end\":{},\"text\":{}}}",
                        quote(&e.file),
                        e.start,
                        e.end,
                        quote(&e.text)
                    );
                }
                s.push_str("]}");
            }
            s.push(']');
        }
        if !self.labels.is_empty() {
            s.push_str(",\"labels\":[");
            for (n, label) in self.labels.iter().enumerate() {
                if n > 0 {
                    s.push(',');
                }
                let _ = write!(
                    s,
                    "{{\"message\":{},\"file\":{},\"line\":{},\"column\":{},\"end_line\":{},\"end_column\":{}}}",
                    quote(&label.message),
                    label
                        .file
                        .as_deref()
                        .map_or_else(|| "null".to_owned(), quote),
                    opt_num(label.line),
                    opt_num(label.column),
                    opt_num(label.end_line),
                    opt_num(label.end_column)
                );
            }
            s.push(']');
        }
        s.push('}');
        s
    }
}

/// What a REPL input shows of its build's diagnostics: every error, since
/// any error stops the input, but only the warnings located in the input
/// itself (`file`, such as `repl#3`). An earlier input's or the package's
/// warnings were shown when that code was built.
#[must_use]
pub fn for_input(diags: Vec<Diag>, file: &str) -> Vec<Diag> {
    diags
        .into_iter()
        .filter(|d| d.is_error() || d.file.as_deref() == Some(file))
        .collect()
}

/// The diagnostics of a compiler run, in content order. `as_written`
/// replaces the file of a single-file program (`cli.diagnostic.file`).
/// No migration edit is among their fix-its.
#[must_use]
pub fn from_output(out: &Output, as_written: Option<&str>) -> Vec<Diag> {
    let d = &out.diags;
    let input = as_written.and_then(|written| std::path::Path::new(written).file_name()?.to_str());
    let file_of = |span: Span| {
        let file = out.locate(span).0;
        (!file.is_empty()).then(|| {
            as_written
                .filter(|_| input == Some(file.as_str()))
                .map_or(file, str::to_owned)
        })
    };
    d.content_order()
        .into_iter()
        .map(|i| {
            // An internal error has no code: its message names it.
            let code = d.shown_code(i);
            let raw = d.get_text(d.message[i]);
            // A message that repeats its code drops it; the record holds it.
            let message = code
                .and_then(|c| raw.strip_prefix(c.as_str()))
                .and_then(|rest| rest.strip_prefix(": "))
                .unwrap_or(raw);
            let span = d.primary[i];
            let file = file_of(span);
            let at = |offset| {
                let (_, line, column) = out.locate(Span { lo: offset, ..span });
                file.as_ref().map(|_| (line, column))
            };
            let (start, end) = (at(span.lo), at(span.hi));
            let fixes = d.fixes[i];
            let fixes = (fixes.start as usize..(fixes.start + fixes.len) as usize)
                .map(|f| Fix {
                    message: out.user_text(d.get_text(d.fix_title[f])),
                    edits: d.edits[d.fix_edits[f].range()]
                        .iter()
                        .map(|e| Edit {
                            file: file_of(e.span).unwrap_or_default(),
                            start: e.span.lo,
                            end: e.span.hi,
                            text: d.get_text(e.text).to_owned(),
                        })
                        .collect(),
                    exact: d.fix_safety[f] == hd_diag::FixSafety::Exact,
                })
                .collect();
            Diag {
                code,
                severity: d.severity[i],
                message: out.user_text(message),
                file,
                line: start.map(|p| p.0),
                column: start.map(|p| p.1),
                end_line: end.map(|p| p.0),
                end_column: end.map(|p| p.1),
                fixes,
                labels: d
                    .labels(i)
                    .iter()
                    .map(|(span, message)| {
                        let file = file_of(*span);
                        let (_, line, column) = out.locate(*span);
                        let (_, end_line, end_column) = out.locate(Span {
                            lo: span.hi,
                            ..*span
                        });
                        Label {
                            message: out.user_text(message),
                            line: file.as_ref().map(|_| line),
                            column: file.as_ref().map(|_| column),
                            end_line: file.as_ref().map(|_| end_line),
                            end_column: file.as_ref().map(|_| end_column),
                            file,
                        }
                    })
                    .collect(),
            }
        })
        .collect()
}

/// Reports a stage's stop at the construct, or at the sentinel span when
/// the stage has no position: a construct this build does not support, as
/// an `unsupported` error, or a failed internal check
/// ([`NotImplemented::bug`]), as an internal error that asks for a report.
pub(crate) fn report_stop(diags: &mut DiagBuf, s: Stage, e: &NotImplemented) {
    let span = e.span.unwrap_or(Span {
        file: FileId::from_raw(u32::MAX),
        lo: 0,
        hi: 0,
    });
    let (name, what) = (s.name(), &e.what);
    if e.bug {
        diags.internal_error(
            span,
            &format!("the compiler hit a bug in stage {name} ({what}); please report it"),
        );
    } else if e.too_large {
        diags.error(Code::TypeTooLarge, span, what);
    } else {
        diags.error(
            Code::Unsupported,
            span,
            &format!("stage {name}: {what} is not supported"),
        );
    }
}

pub use hd_diag::quote;

/// `diags` as bytes that [`decode`] reads back equal, for a caller that
/// stores a report under a [`crate::ResultKey`].
#[must_use]
pub fn encode(diags: &[Diag]) -> Vec<u8> {
    use hd_base::wire::Writer;
    let opt = |w: &mut Writer, v: Option<usize>| match v {
        None => w.u8(0),
        Some(v) => {
            w.u8(1);
            w.u64(v as u64);
        }
    };
    let mut w = Writer::default();
    w.len_of(diags);
    for d in diags {
        w.str(d.code.map_or("", Code::as_str));
        w.u8(u8::from(d.severity == Severity::Warning));
        w.str(&d.message);
        match &d.file {
            None => w.u8(0),
            Some(f) => {
                w.u8(1);
                w.str(f);
            }
        }
        for v in [d.line, d.column, d.end_line, d.end_column] {
            opt(&mut w, v);
        }
        w.len_of(&d.fixes);
        for f in &d.fixes {
            w.str(&f.message);
            w.u8(u8::from(f.exact));
            w.len_of(&f.edits);
            for e in &f.edits {
                w.str(&e.file);
                w.u32(e.start);
                w.u32(e.end);
                w.str(&e.text);
            }
        }
        w.len_of(&d.labels);
        for label in &d.labels {
            w.str(&label.message);
            match &label.file {
                None => w.u8(0),
                Some(file) => {
                    w.u8(1);
                    w.str(file);
                }
            }
            for value in [label.line, label.column, label.end_line, label.end_column] {
                opt(&mut w, value);
            }
        }
    }
    w.bytes
}

/// The diagnostics [`encode`] wrote, or `None` for damaged bytes.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<Vec<Diag>> {
    use hd_base::wire::Reader;
    let opt = |r: &mut Reader<'_>| match r.u8() {
        0 => None,
        _ => usize::try_from(r.u64()).ok(),
    };
    let mut r = Reader::new(bytes);
    let mut out = Vec::new();
    for _ in 0..r.count() {
        let code = match r.str() {
            "" => None,
            name => Some(Code::from_name(name)?),
        };
        let severity = if r.u8() == 0 {
            Severity::Error
        } else {
            Severity::Warning
        };
        let message = r.str().to_owned();
        let file = (r.u8() != 0).then(|| r.str().to_owned());
        let (line, column, end_line, end_column) =
            (opt(&mut r), opt(&mut r), opt(&mut r), opt(&mut r));
        let fixes = (0..r.count())
            .map(|_| Fix {
                message: r.str().to_owned(),
                exact: r.u8() != 0,
                edits: (0..r.count())
                    .map(|_| Edit {
                        file: r.str().to_owned(),
                        start: r.u32(),
                        end: r.u32(),
                        text: r.str().to_owned(),
                    })
                    .collect(),
            })
            .collect();
        let labels = (0..r.count())
            .map(|_| Label {
                message: r.str().to_owned(),
                file: (r.u8() != 0).then(|| r.str().to_owned()),
                line: opt(&mut r),
                column: opt(&mut r),
                end_line: opt(&mut r),
                end_column: opt(&mut r),
            })
            .collect();
        out.push(Diag {
            code,
            severity,
            message,
            file,
            line,
            column,
            end_line,
            end_column,
            fixes,
            labels,
        });
    }
    (r.ok() && r.at_end()).then_some(out)
}
