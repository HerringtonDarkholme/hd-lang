//! `hd check [FILE] [--format json]` (`cli.package.whole`, `cli.check.*`,
//! `cli.file.check-test`, `cli.json.*`): the pipeline through checking and
//! coherence, with no collection or emission.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use hd_cache::DiskStore;
use hd_diag::Code;
use hd_driver::{Goal, Host, Output, build_packages};

use crate::report::{self, Diag, Summary};
use crate::{HD_FAILURE, Wall, cache_dir, disk, executor, fail, package_of_cwd};

/// What `hd check` was asked for.
struct Options {
    file: Option<OsString>,
    json: bool,
    summary: bool,
    max_errors: Option<usize>,
}

fn options(args: &[OsString]) -> Result<Options, String> {
    let mut o = Options {
        file: None,
        json: false,
        summary: false,
        max_errors: None,
    };
    let mut rest = args.iter();
    while let Some(a) = rest.next() {
        let text = a.to_string_lossy();
        if text == "--summary" {
            o.summary = true;
            continue;
        }
        let max = if text == "--max-errors" {
            let value = rest.next().ok_or("`--max-errors` needs a value")?;
            Some(value.to_string_lossy().into_owned())
        } else {
            text.strip_prefix("--max-errors=").map(str::to_owned)
        };
        if let Some(value) = max {
            match value.parse::<usize>() {
                Ok(n) if n >= 1 => o.max_errors = Some(n),
                _ => {
                    return Err(format!(
                        "`--max-errors` takes a whole number of at least 1, not `{value}`"
                    ));
                }
            }
            continue;
        }
        let format = if text == "--format" {
            let value = rest.next().ok_or("`--format` needs a value")?;
            Some(value.to_string_lossy().into_owned())
        } else {
            text.strip_prefix("--format=").map(str::to_owned)
        };
        if let Some(value) = format {
            match value.as_str() {
                "json" => o.json = true,
                "text" => o.json = false,
                _ => return Err(format!("`--format` takes `text` or `json`, not `{value}`")),
            }
        } else if text.starts_with('-') {
            return Err(format!("`hd check` has no option `{text}`"));
        } else if o.file.replace(a.clone()).is_some() {
            return Err("`hd check` takes at most one FILE".to_owned());
        }
    }
    Ok(o)
}

/// A program to check, and the path to show for a single file
/// (`cli.json.diagnostic.file`).
struct Target {
    program: disk::Program,
    as_written: Option<String>,
    /// The package-relative FILE whose module is checked (`cli.package.file`).
    only: Option<String>,
}

fn target(file: Option<&OsString>) -> Result<Target, String> {
    let Some(file) = file else {
        let root = package_of_cwd("check")
            .map_err(|e| format!("{e}; or pass a FILE to check it as a single-file program"))?;
        return Ok(Target {
            program: disk::load_package(&root, "main")?,
            as_written: None,
            only: None,
        });
    };
    let path = Path::new(file);
    if path.extension().is_none_or(|x| x != "hd") {
        return Err(format!(
            "`{}` is not an .hd file; `hd check` takes a FILE.hd",
            path.display()
        ));
    }
    let full = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let dir = full.parent().unwrap_or(Path::new("."));
    match disk::package_root(dir) {
        Some(root) => {
            let only = full.strip_prefix(&root).ok().map(|rel| {
                rel.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            });
            Ok(Target {
                program: disk::load_package(&root, "main")?,
                as_written: None,
                only,
            })
        }
        None => Ok(Target {
            program: disk::load_file(path)?,
            as_written: Some(path.to_string_lossy().into_owned()),
            only: None,
        }),
    }
}

pub(crate) fn command(args: &[OsString]) -> ExitCode {
    let o = match options(args) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    let (diags, modules_checked) = match target(o.file.as_ref()) {
        Ok(t) if t.program.problems.iter().any(Diag::is_error) => (t.program.problems, 0),
        Ok(t) => {
            let store = DiskStore { root: cache_dir() };
            let clock = Wall(Instant::now());
            let host = Host {
                render_tir: &[],
                sources: &t.program.sources,
                store: &store,
                clock: &clock,
                executor: executor(),
            };
            let out = build_packages(
                &host,
                &t.program.package,
                &t.program.packages(),
                &Goal::Analyze,
            );
            let shown = shown(&out, &t);
            let mut diags = t.program.problems;
            diags.extend(shown);
            (diags, out.counters.modules_checked.len())
        }
        Err(e) => (vec![Diag::error(None, &e)], 0),
    };
    let failed = diags.iter().any(Diag::is_error);
    let status = if failed { HD_FAILURE } else { 0 };
    if o.json {
        let mut text = String::new();
        for d in &diags {
            let _ = writeln!(text, "{}", d.json());
        }
        let summary = Summary {
            status,
            modules_checked: Some(modules_checked),
            ..Summary::of(&diags)
        };
        let _ = writeln!(text, "{}", summary.json());
        print!("{text}");
    } else {
        eprint!("{}", text_report(&diags, &o));
    }
    ExitCode::from(status)
}

/// The diagnostics this run reports, in content order. A FILE in a package
/// narrows them to its module and the modules it uses, deeply
/// (`cli.package.file`); a diagnostic of no file is kept.
fn shown(out: &Output, t: &Target) -> Vec<Diag> {
    let all = report::from_output(out, &t.program.sources, t.as_written.as_deref());
    let Some(only) = &t.only else {
        return all;
    };
    let mut reached = vec![false; out.files.len()];
    let mut stack: Vec<usize> = out
        .files
        .iter()
        .position(|f| f == only)
        .into_iter()
        .collect();
    while let Some(f) = stack.pop() {
        if !std::mem::replace(&mut reached[f], true) {
            stack.extend(out.uses.get(f).into_iter().flatten().copied());
        }
    }
    all.into_iter()
        .filter(|d| {
            d.file.as_ref().is_none_or(|file| {
                out.files
                    .iter()
                    .position(|f| f == file)
                    .is_some_and(|i| reached[i])
            })
        })
        .collect()
}

/// The text report of `cli.check.*`: each diagnostic, or with `--summary` one
/// line per severity, file, and code, then the `check result` line.
fn text_report(diags: &[Diag], o: &Options) -> String {
    let mut text = String::new();
    let (mut errors, mut warnings) = (0, 0);
    let mut stopped = false;
    let mut counts: BTreeMap<(&str, &str, &str), usize> = BTreeMap::new();
    for d in diags {
        if d.is_error() {
            errors += 1;
        } else {
            warnings += 1;
        }
        if o.summary {
            let file = d.file.as_deref().unwrap_or("");
            let code = d.code.map_or("", Code::as_str);
            *counts.entry((file, code, d.severity_word())).or_insert(0) += 1;
            continue;
        }
        if stopped {
            continue;
        }
        let _ = writeln!(text, "{}", d.text());
        // Printing stops after the Nth error; the counts keep going.
        stopped = d.is_error() && o.max_errors.is_some_and(|n| errors >= n);
    }
    for ((file, code, severity), n) in &counts {
        let _ = writeln!(text, "{severity}: {file}: {code}: {n}");
    }
    let result = if errors > 0 { "FAILED" } else { "ok" };
    let _ = writeln!(
        text,
        "check result: {result}. errors: {errors}; warnings: {warnings}"
    );
    text
}
