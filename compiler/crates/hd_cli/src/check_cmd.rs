//! `hd check [FILE] [--format json]` (`cli.package.whole`, `cli.check.*`,
//! `cli.file.check-test`, `cli.json.*`): the pipeline through checking and
//! coherence, with no collection or emission.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use hd_cache::DiskStore;
use hd_diag::Severity;
use hd_driver::{Goal, Host, Output, build};

use crate::{HD_FAILURE, Wall, cache_dir, disk, executor, fail, package_of_cwd};

/// What `hd check` was asked for.
struct Options {
    file: Option<OsString>,
    json: bool,
}

fn options(args: &[OsString]) -> Result<Options, String> {
    let mut o = Options {
        file: None,
        json: false,
    };
    let mut rest = args.iter();
    while let Some(a) = rest.next() {
        let text = a.to_string_lossy();
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
}

fn target(file: Option<&OsString>) -> Result<Target, String> {
    let Some(file) = file else {
        let root = package_of_cwd("check")
            .map_err(|e| format!("{e}; or pass a FILE to check it as a single-file program"))?;
        return Ok(Target {
            program: disk::load_package(&root, "main")?,
            as_written: None,
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
        Some(root) => Ok(Target {
            program: disk::load_package(&root, "main")?,
            as_written: None,
        }),
        None => Ok(Target {
            program: disk::load_file(path)?,
            as_written: Some(path.to_string_lossy().into_owned()),
        }),
    }
}

pub(crate) fn command(args: &[OsString]) -> ExitCode {
    let o = match options(args) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    let t = match target(o.file.as_ref()) {
        Ok(t) => t,
        Err(e) => return fail(&e),
    };
    let store = DiskStore { root: cache_dir() };
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &t.program.sources,
        store: &store,
        clock: &clock,
        executor: executor(),
    };
    let out = build(&host, &t.program.package, &Goal::Analyze);
    let failed = out.diags.has_errors();
    if o.json {
        print!("{}", json_lines(&out, &t, failed));
    } else {
        eprint!("{}", out.render_located(&t.program.sources));
    }
    if failed {
        ExitCode::from(HD_FAILURE)
    } else {
        ExitCode::SUCCESS
    }
}

/// The JSON lines of `cli.json.*`: each diagnostic, then the summary.
fn json_lines(out: &Output, t: &Target, failed: bool) -> String {
    let d = &out.diags;
    let name = |file: String| t.as_written.clone().unwrap_or(file);
    let mut text = String::new();
    let (mut errors, mut warnings) = (0, 0);
    for i in d.content_order() {
        let severity = match d.severity[i] {
            Severity::Error => {
                errors += 1;
                "error"
            }
            Severity::Warning => {
                warnings += 1;
                "warning"
            }
        };
        let (file, line, column) = out.locate(&t.program.sources, d.primary[i]);
        let _ = write!(
            text,
            "{{\"kind\":\"diagnostic\",\"code\":\"{}\",\"severity\":\"{severity}\",\"message\":{},\"file\":{},\"line\":{line},\"column\":{column},\"fixes\":[",
            d.code[i].as_str(),
            quote(d.get_text(d.message[i])),
            quote(&name(file)),
        );
        let fixes = d.fixes[i];
        for (n, f) in (fixes.start as usize..(fixes.start + fixes.len) as usize).enumerate() {
            if n > 0 {
                text.push(',');
            }
            let _ = write!(
                text,
                "{{\"message\":{},\"edits\":[",
                quote(d.get_text(d.fix_title[f]))
            );
            let edits = d.fix_edits[f];
            for (m, e) in d.edits[edits.range()].iter().enumerate() {
                if m > 0 {
                    text.push(',');
                }
                let (file, _, _) = out.locate(&t.program.sources, e.span);
                let _ = write!(
                    text,
                    "{{\"file\":{},\"start\":{},\"end\":{},\"text\":{}}}",
                    quote(&name(file)),
                    e.span.lo,
                    e.span.hi,
                    quote(d.get_text(e.text))
                );
            }
            text.push_str("]}");
        }
        text.push_str("]}\n");
    }
    let status = if failed { u32::from(HD_FAILURE) } else { 0 };
    let _ = writeln!(
        text,
        "{{\"kind\":\"summary\",\"errors\":{errors},\"warnings\":{warnings},\"passed\":0,\"failed\":0,\"ignored\":0,\"status\":{status},\"modules_checked\":{}}}",
        out.counters.modules_checked.len()
    );
    text
}

/// A JSON string literal.
fn quote(s: &str) -> String {
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
