//! Diagnostics as the CLI and the playground report them: one record per
//! diagnostic, whatever found it (a manifest rule, the command line, or the
//! compiler), written as text (`severity: file:line:column: code: message`)
//! or as a JSON line (`cli.json.diagnostic.*`, `cli.json.fix.*`). Messages
//! name test and task code by file (`module.test.integration.no-path`,
//! `cli.task.no-path`).

use std::fmt::Write as _;

use hd_base::Span;
use hd_diag::{Code, Severity};

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
/// offers it as the whole fix, so `hd fix` tries it (`cli.fix.safe`); a
/// suggestion is only shown. The JSON line leaves it out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub message: String,
    pub edits: Vec<Edit>,
    pub exact: bool,
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
    /// out each part the diagnostic lacks.
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
        s
    }

    /// The JSON line (`cli.json.diagnostic.fields`, `cli.json.diagnostic.fixes`).
    #[must_use]
    pub fn json(&self) -> String {
        let opt_num = |n: Option<usize>| n.map_or_else(|| "null".to_owned(), |n| n.to_string());
        let mut s = format!(
            "{{\"kind\":\"diagnostic\",\"code\":{},\"severity\":\"{}\",\"message\":{},\"file\":{},\"line\":{},\"column\":{},\"fixes\":[",
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
        for (n, f) in self.fixes.iter().enumerate() {
            if n > 0 {
                s.push(',');
            }
            let _ = write!(s, "{{\"message\":{},\"edits\":[", quote(&f.message));
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
        s.push_str("]}");
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
/// replaces the file of a single-file program (`cli.json.diagnostic.file`).
#[must_use]
pub fn from_output(out: &Output, as_written: Option<&str>) -> Vec<Diag> {
    let d = &out.diags;
    let file_of = |span: Span| {
        let file = out.locate(span).0;
        (!file.is_empty()).then(|| as_written.map_or(file, str::to_owned))
    };
    d.content_order()
        .into_iter()
        .map(|i| {
            let code = d.code[i];
            let raw = d.get_text(d.message[i]);
            // A message that repeats its code drops it; the record holds it.
            let message = raw
                .strip_prefix(code.as_str())
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
                code: Some(code),
                severity: d.severity[i],
                message: out.user_text(message),
                file,
                line: start.map(|p| p.0),
                column: start.map(|p| p.1),
                end_line: end.map(|p| p.0),
                end_column: end.map(|p| p.1),
                fixes,
            }
        })
        .collect()
}

/// A JSON string literal.
#[must_use]
pub fn quote(s: &str) -> String {
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
