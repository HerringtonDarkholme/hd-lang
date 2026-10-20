//! How `hd` reports: the diagnostic records of `hd_driver::diagnostics`,
//! which the playground shares, written as text or JSON lines to the
//! command's stream, and the JSON summary (`cli.json.summary.*`).

use std::fmt::Write as _;

pub(crate) use hd_driver::diagnostics::{Diag, Edit, Fix, from_output, quote};

/// Renames the files of diagnostics and their fix-its, as a package with
/// another source root names them on disk (`DiskSources::display`).
pub(crate) fn relocate(diags: &mut [Diag], display: impl Fn(&str) -> String) {
    for d in diags {
        if let Some(f) = &mut d.file {
            *f = display(f);
        }
        for e in d.fixes.iter_mut().flat_map(|f| &mut f.edits) {
            e.file = display(&e.file);
        }
    }
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

    /// `fail` for a package command, whose error line in text mode names
    /// the command: `hd add: text is in [dependencies]; ...`.
    pub(crate) fn command_fail(&mut self, command: &str, message: &str) -> std::process::ExitCode {
        if self.json {
            return self.fail(message);
        }
        self.errors += 1;
        self.line(&format!("hd {command}: {message}"));
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
