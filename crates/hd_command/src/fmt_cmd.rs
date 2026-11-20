//! `hd fmt [--check] [FILE]` (Formatting, `cli.fmt.*`): each `.hd` file of
//! the package, or FILE alone, through `hd_fmt::format`. No other module is
//! read and no type is checked (`cli.fmt.no-check`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hd_base::lines::LineIndex;
use hd_project::SourceSet as _;

use crate::report::Diag;
use crate::{HD_FAILURE, disk, fail};

pub(crate) fn command(args: &[OsString]) -> ExitCode {
    let mut check = false;
    let mut file: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        // `cli.jobs` is accepted; files are formatted one by one.
        if let Some(jobs) = crate::jobs_flag(args, i) {
            match jobs {
                Ok((_, used)) => i += used,
                Err(e) => return fail(&e),
            }
            continue;
        }
        let a = &args[i];
        i += 1;
        let text = a.to_string_lossy();
        if text == "--check" {
            check = true;
        } else if text.starts_with('-') {
            return fail(&format!("`hd fmt` has no option `{text}`"));
        } else if file.replace(PathBuf::from(a)).is_some() {
            return fail("`hd fmt` takes at most one FILE");
        }
    }
    // Each file to format: its path on disk and the path to show.
    let files: Vec<(PathBuf, String)> = if let Some(f) = file {
        // `cli.fmt.file`: FILE alone, in a package or outside one.
        let shown = f.to_string_lossy().into_owned();
        vec![(f, shown)]
    } else {
        let cwd = match std::env::current_dir() {
            Ok(c) => c,
            Err(e) => return fail(&e.to_string()),
        };
        let Some(root) = disk::package_root(&cwd) else {
            return fail(
                "outside any package, `hd fmt` needs a FILE; pass one, or create a package with `hd new`",
            );
        };
        // `cli.fmt.package`: the source root, the test root and `tasks`.
        match disk::sources_of(&root) {
            Ok((sources, _)) => sources
                .list()
                .into_iter()
                .map(|e| (sources.disk_path(&e.path), sources.display(&e.path)))
                .collect(),
            Err(e) => return fail(&e),
        }
    };
    let mut failed = false;
    let mut unformatted = false;
    for (path, shown) in files {
        match format_one(&path, &shown, check) {
            Ok(changed) => unformatted |= changed,
            Err(diags) => {
                failed = true;
                for d in diags {
                    eprintln!("{}", d.text());
                }
            }
        }
    }
    if failed {
        ExitCode::from(HD_FAILURE)
    } else if check && unformatted {
        // `cli.exit.fmt-check`.
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Formats one file: whether formatting changes it, or the diagnostics
/// that leave it untouched (`cli.fmt.syntax-error`). `--check` prints the
/// path of a file that would change and writes nothing (`cli.fmt.check`);
/// otherwise a changed file is replaced whole, through a temporary file.
fn format_one(path: &Path, shown: &str, check: bool) -> Result<bool, Vec<Diag>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| vec![Diag::error(None, &format!("{shown}: {e}"))])?;
    let formatted = hd_fmt::format(&text).map_err(|diags| syntax_errors(&text, shown, &diags))?;
    if formatted == text {
        return Ok(false);
    }
    if check {
        println!("{shown}");
        return Ok(true);
    }
    let temp = path.with_extension("hd.fmt-tmp");
    std::fs::write(&temp, &formatted)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|e| vec![Diag::error(None, &format!("{shown}: {e}"))])?;
    Ok(true)
}

/// The diagnostics of a file that does not parse, each at its line and
/// column through one line index of the file.
fn syntax_errors(text: &str, shown: &str, diags: &[hd_diag::Diagnostic]) -> Vec<Diag> {
    let lines = LineIndex::new(text.as_bytes());
    diags
        .iter()
        .map(|d| {
            let (line, column) = lines.line_column((d.primary.lo as usize).min(text.len()));
            let mut out = Diag::error(
                Some(d.code),
                "the file does not parse, so `hd fmt` leaves it as it is",
            )
            .at(shown, Some(line));
            out.column = Some(column);
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    /// #1850: a file of n syntax errors is located through one line
    /// index, not one scan from its start per error.
    #[test]
    fn syntax_errors_index_their_file_once() {
        for n in [2000, 20_000] {
            let mut text = String::new();
            for i in 0..n {
                let _ = writeln!(text, "fn f{i}( -> i32: 1");
            }
            let diags = hd_fmt::format(&text).expect_err("does not parse");
            let (errors, indexed) =
                hd_base::lines::count_indexed_bytes(|| super::syntax_errors(&text, "a.hd", &diags));
            assert_eq!(indexed, text.len() as u64, "{n} errors");
            assert_eq!(errors.len(), diags.len());
            assert!(errors.len() >= n, "{} errors", errors.len());
            assert_eq!(errors.last().and_then(|d| d.line), Some(n));
        }
    }
}
