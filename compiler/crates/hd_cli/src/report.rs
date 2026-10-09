//! Diagnostics as `hd` reports them: one record per diagnostic, whatever
//! found it (a manifest rule, the command line, or the compiler), written
//! as text (`severity: file:line:column: code: message`) or as a JSON line
//! (`cli.json.diagnostic.*`, `cli.json.fix.*`), and the JSON summary
//! (`cli.json.summary.*`). Messages name test and task code by file
//! (`module.test.integration.no-path`, `cli.task.no-path`).

use std::fmt::Write as _;

use hd_diag::{Code, Severity};
use hd_driver::Output;
use hd_project::SourceSet;

/// One edit of a fix-it: bytes `start..end` of `file` become `text`.
pub(crate) struct Edit {
    pub(crate) file: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) text: String,
}

pub(crate) struct Fix {
    pub(crate) message: String,
    pub(crate) edits: Vec<Edit>,
}

/// One diagnostic. A command-line error has no code; a diagnostic that no
/// source text causes has no file (`cli.json.diagnostic.no-position`).
pub(crate) struct Diag {
    pub(crate) code: Option<Code>,
    pub(crate) severity: Severity,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<usize>,
    pub(crate) column: Option<usize>,
    pub(crate) fixes: Vec<Fix>,
}

impl Diag {
    pub(crate) fn error(code: Option<Code>, message: &str) -> Diag {
        Diag {
            code,
            severity: Severity::Error,
            message: message.to_owned(),
            file: None,
            line: None,
            column: None,
            fixes: Vec::new(),
        }
    }

    /// The same diagnostic at `file`, and at `line` when it has one.
    pub(crate) fn at(mut self, file: &str, line: Option<usize>) -> Diag {
        self.file = Some(file.to_owned());
        self.line = line;
        self.column = line.map(|_| 1);
        self
    }

    pub(crate) fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub(crate) fn severity_word(&self) -> &'static str {
        match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }

    /// The text line: `severity: file:line:column: code: message`, leaving
    /// out each part the diagnostic lacks.
    pub(crate) fn text(&self) -> String {
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
    pub(crate) fn json(&self) -> String {
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

/// The diagnostics of a compiler run, in content order. `as_written`
/// replaces the file of a single-file program (`cli.json.diagnostic.file`).
pub(crate) fn from_output(
    out: &Output,
    sources: &dyn SourceSet,
    as_written: Option<&str>,
) -> Vec<Diag> {
    let d = &out.diags;
    let place = |span| {
        let (file, line, column) = out.locate(sources, span);
        if file.is_empty() {
            (None, None, None)
        } else {
            let file = as_written.map_or(file, str::to_owned);
            (Some(file), Some(line), Some(column))
        }
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
            let (file, line, column) = place(d.primary[i]);
            let fixes = d.fixes[i];
            let fixes = (fixes.start as usize..(fixes.start + fixes.len) as usize)
                .map(|f| Fix {
                    message: out.user_text(d.get_text(d.fix_title[f])),
                    edits: d.edits[d.fix_edits[f].range()]
                        .iter()
                        .map(|e| Edit {
                            file: place(e.span).0.unwrap_or_default(),
                            start: e.span.lo,
                            end: e.span.hi,
                            text: d.get_text(e.text).to_owned(),
                        })
                        .collect(),
                })
                .collect();
            Diag {
                code: Some(code),
                severity: d.severity[i],
                message: out.user_text(message),
                file,
                line,
                column,
                fixes,
            }
        })
        .collect()
}

/// The counts of a JSON summary (`cli.json.summary.result.fields`).
#[derive(Default)]
pub(crate) struct Summary {
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
    pub(crate) passed: usize,
    pub(crate) failed: usize,
    pub(crate) ignored: usize,
    pub(crate) status: u8,
    /// `hd check` only (`cli.json.summary.modules-checked`).
    pub(crate) modules_checked: Option<usize>,
}

impl Summary {
    /// The errors and warnings of `diags`, counted.
    pub(crate) fn of(diags: &[Diag]) -> Summary {
        let errors = diags.iter().filter(|d| d.is_error()).count();
        Summary {
            errors,
            warnings: diags.len() - errors,
            ..Summary::default()
        }
    }

    pub(crate) fn json(&self) -> String {
        let mut s = format!(
            "{{\"kind\":\"summary\",\"errors\":{},\"warnings\":{},\"passed\":{},\"failed\":{},\"ignored\":{},\"status\":{}",
            self.errors, self.warnings, self.passed, self.failed, self.ignored, self.status
        );
        if let Some(n) = self.modules_checked {
            let _ = write!(s, ",\"modules_checked\":{n}");
        }
        s.push('}');
        s
    }
}

/// A JSON string literal.
pub(crate) fn quote(s: &str) -> String {
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

/// Where a command writes its diagnostics and summary: text lines on
/// standard error, or with `--format json` JSON lines on standard output
/// (`cli.json.lines.build`) or, for `hd run` and `hd FILE`, standard error
/// (`cli.json.run`). Text mode writes no summary here; each command prints
/// its own.
pub(crate) struct Reporter {
    pub(crate) json: bool,
    to_stderr: bool,
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
}

impl Reporter {
    /// A reporter for `hd build`, `hd check` and `hd test`.
    pub(crate) fn stdout(json: bool) -> Reporter {
        Reporter {
            json,
            to_stderr: false,
            errors: 0,
            warnings: 0,
        }
    }

    /// A reporter for `hd run` and `hd FILE`, whose standard output is the
    /// program's (`cli.json.run.program`).
    pub(crate) fn stderr(json: bool) -> Reporter {
        Reporter {
            to_stderr: true,
            ..Reporter::stdout(json)
        }
    }

    /// Writes one JSON line, or in text mode one text line, to the stream
    /// of this command's JSON (text always goes to standard error).
    fn line(&self, json: &str) {
        if self.to_stderr || !self.json {
            eprintln!("{json}");
        } else {
            println!("{json}");
        }
    }

    pub(crate) fn diag(&mut self, d: &Diag) {
        if d.is_error() {
            self.errors += 1;
        } else {
            self.warnings += 1;
        }
        self.line(&if self.json { d.json() } else { d.text() });
    }

    /// Each diagnostic in order; whether any is an error.
    pub(crate) fn diags(&mut self, ds: &[Diag]) -> bool {
        for d in ds {
            self.diag(d);
        }
        ds.iter().any(Diag::is_error)
    }

    /// A JSON line that is no diagnostic, such as a test object; nothing in
    /// text mode.
    pub(crate) fn json_line(&self, json: &str) {
        if self.json {
            self.line(json);
        }
    }

    /// An error with no code, such as a rejected command line, and the end
    /// of the command with status 101 (`cli.exit.hd-failure`).
    pub(crate) fn fail(&mut self, message: &str) -> std::process::ExitCode {
        self.diag(&Diag::error(None, message));
        self.finish(crate::HD_FAILURE)
    }

    /// The end of the command: in JSON mode its summary
    /// (`cli.json.summary.result`), then `status`.
    pub(crate) fn finish(&mut self, status: u8) -> std::process::ExitCode {
        self.finish_tests(status, 0, 0, 0)
    }

    /// `finish`, with the test counts of `hd test`.
    pub(crate) fn finish_tests(
        &mut self,
        status: u8,
        passed: usize,
        failed: usize,
        ignored: usize,
    ) -> std::process::ExitCode {
        let s = Summary {
            errors: self.errors,
            warnings: self.warnings,
            passed,
            failed,
            ignored,
            status,
            modules_checked: None,
        };
        self.json_line(&s.json());
        std::process::ExitCode::from(status)
    }
}

/// Reads `--format VALUE` or `--format=VALUE` at `args[i]`: `Some(json)` and
/// how many words it took, or `None` when `args[i]` is another word.
pub(crate) fn format_flag(
    args: &[std::ffi::OsString],
    i: usize,
) -> Option<Result<(bool, usize), String>> {
    let text = args.get(i)?.to_string_lossy();
    let (value, used) = if text == "--format" {
        match args.get(i + 1) {
            Some(v) => (v.to_string_lossy().into_owned(), 2),
            None => return Some(Err("`--format` needs a value".to_owned())),
        }
    } else {
        (text.strip_prefix("--format=")?.to_owned(), 1)
    };
    Some(match value.as_str() {
        "json" => Ok((true, used)),
        "text" => Ok((false, used)),
        _ => Err(format!("`--format` takes `text` or `json`, not `{value}`")),
    })
}
