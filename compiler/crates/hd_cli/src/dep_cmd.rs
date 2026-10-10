//! The dependency commands (Dependency Commands, `cli.dep.*`):
//! `hd remove NAME` and `hd fetch`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hd_diag::{Code, Severity};

use crate::report::{Diag, Reporter, format_flag};
use crate::{HD_FAILURE, disk, fail};

/// The package a dependency command edits (`cli.dep.package-only`,
/// `cli.dep.workspace-member-only`).
fn package(command: &str) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    if crate::disk::workspace_root(&cwd).is_some() {
        return Err(format!(
            "`hd {command}` edits one package's requirements, and this is a workspace root; run it in a member's directory"
        ));
    }
    crate::disk::package_root(&cwd).ok_or_else(|| {
        format!(
            "`hd {command}` works on a package, and no `hd.toml` is at or above here; create a package with `hd new`"
        )
    })
}

/// The words of a dependency command, with `--format` taken out
/// (`cli.json.commands`).
fn words(args: &[OsString]) -> Result<(bool, Vec<String>), String> {
    let mut json = false;
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(format) = format_flag(args, i) {
            let (j, used) = format?;
            json = j;
            i += used;
            continue;
        }
        out.push(args[i].to_string_lossy().into_owned());
        i += 1;
    }
    Ok((json, out))
}

/// `hd remove NAME` (`cli.dep.remove`, `cli.dep.edit`,
/// `cli.dep.edit.empty-table`, `cli.dep.unchanged-on-error`).
pub(crate) fn remove(args: &[OsString]) -> ExitCode {
    let (json, words) = match words(args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stdout(json);
    let name = match words.as_slice() {
        [n] if !n.starts_with('-') => n.clone(),
        [n, ..] if n.starts_with('-') => {
            return rep.fail(&format!("`hd remove` has no option `{n}`"));
        }
        _ => return rep.fail("`hd remove` takes one NAME"),
    };
    let root = match package("remove") {
        Ok(r) => r,
        Err(e) => return rep.fail(&e),
    };
    match remove_in(&root, &name) {
        Ok(()) => rep.finish(0),
        Err(e) => rep.fail(&e),
    }
}

fn remove_in(root: &Path, name: &str) -> Result<(), String> {
    let manifest = root.join("hd.toml");
    let text =
        std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let m = hd_project::parse_manifest(&text).map_err(|e| format!("hd.toml: {e}"))?;
    let (table, req) = [
        ("dependencies", &m.dependencies),
        ("dev-dependencies", &m.dev_dependencies),
    ]
    .into_iter()
    .find_map(|(t, reqs)| reqs.iter().find(|r| r.key == name).map(|r| (t, r)))
    .ok_or_else(|| {
        format!("`{name}` is no key of `[dependencies]` or `[dev-dependencies]` in hd.toml")
    })?;
    let edited = hd_project::edit::remove_key(&text, table, name)
        .ok_or_else(|| format!("hd.toml: cannot find the line of `{name}`"))?;
    // `cli.dep.tidy`, as far as no fetch is needed: the removed version's
    // entries go unless another requirement names the same version.
    let sum_path = root.join("hd.sum");
    let sum = std::fs::read_to_string(&sum_path).ok();
    let tidy = sum.as_ref().and_then(|sum| {
        let gone = req.host().map(|_| req.text.clone()).filter(|_| {
            !m.dependencies
                .iter()
                .chain(&m.dev_dependencies)
                .any(|r| r.key != name && r.text == req.text)
        })?;
        let mut kept = String::new();
        for l in sum.lines() {
            let entry = l.split(' ').next().unwrap_or("");
            if entry != gone && entry != format!("{gone}/hd.toml") {
                kept.push_str(l);
                kept.push('\n');
            }
        }
        Some(kept)
    });
    std::fs::write(&manifest, edited).map_err(|e| format!("{}: {e}", manifest.display()))?;
    if let Some(kept) = tidy {
        std::fs::write(&sum_path, kept).map_err(|e| format!("{}: {e}", sum_path.display()))?;
    }
    Ok(())
}

/// `hd fetch` (`cli.dep.fetch`, `cli.dep.workspace-fetch`): selects for the
/// package, or for every member of its workspace, fetches each version
/// the cache lacks, and adds each `hd.sum` line the selection needs and
/// the file lacks. It changes no entry (`cli.sum.keep`), and on an error
/// it writes nothing (`cli.dep.unchanged-on-error`). With only path
/// requirements there is no line to add, and no `hd.sum` is written
/// (`cli.dep.path`).
pub(crate) fn fetch(args: &[OsString]) -> ExitCode {
    let (json, words) = match words(args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stdout(json);
    if let Some(w) = words.first() {
        return rep.fail(&format!("`hd fetch` takes no operand or option `{w}`"));
    }
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => return rep.fail(&e.to_string()),
    };
    let (shown, sum_dir, members) = if let Some((ws, members)) = disk::workspace_root(&cwd) {
        (ws.clone(), ws, members)
    } else if let Some(root) = disk::package_root(&cwd) {
        let (sum_dir, members) = disk::selection_start(&root);
        (root, sum_dir, members)
    } else {
        return rep.fail(
            "`hd fetch` works on a package, and no `hd.toml` is at or above here; create a package with `hd new`",
        );
    };
    let shown = std::fs::canonicalize(&shown).unwrap_or(shown);
    // Each member's requirements that break a rule of the manifest alone
    // (`cli.dep.invalid`), which selection leaves out.
    let mut diags = Vec::new();
    for dir in &members {
        let path = dir.join("hd.toml");
        let m = match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| hd_project::parse_manifest(&t).map_err(|e| e.to_string()))
        {
            Ok(m) => m,
            Err(e) => return rep.fail(&format!("{}: {e}", path.display())),
        };
        let file = disk::shown_from(&shown, &std::fs::canonicalize(&path).unwrap_or(path));
        let own = hd_project::problems(&m, disk::TOOLCHAIN);
        for p in &own {
            if p.code == Some(Code::InvalidRequirement) && p.severity == Severity::Error {
                diags.push(Diag::error(p.code, &p.message).at(&file, p.line.map(|l| l as usize)));
            }
        }
        // A path requirement names a package (`module.path-dep.no-package`).
        for r in m.dependencies.iter().chain(&m.dev_dependencies) {
            let Some(path) = &r.path else { continue };
            if own.iter().any(|p| p.line == Some(r.line)) {
                continue;
            }
            match disk::declares_package(&dir.join(path)) {
                Ok(true) => {}
                Ok(false) => diags.push(
                    Diag::error(
                        Some(Code::InvalidRequirement),
                        &format!(
                            "`{}` requires the directory `{path}`, which holds no package: it has no `hd.toml` with a `[package]` section",
                            r.key
                        ),
                    )
                    .at(&file, Some(r.line as usize)),
                ),
                Err(e) => return rep.fail(&e),
            }
        }
    }
    if rep.diags(&diags) {
        return rep.finish(HD_FAILURE);
    }
    let sum = match disk::read_sum(&sum_dir) {
        Ok(s) => s,
        Err((line, message)) => {
            let file = disk::shown_from(&shown, &sum_dir.join("hd.sum"));
            rep.diag(&Diag::error(None, &message).at(&file, Some(line as usize)));
            return rep.finish(HD_FAILURE);
        }
    };
    let mut progress = |key: &str| eprintln!("hd: fetching {key}");
    let mut remote = hd_deps::Remote::new(hd_deps::Cache::new(crate::cache_root()), &mut progress);
    let sel = hd_deps::select(
        &members,
        &sum,
        hd_deps::Mode::Record,
        &mut remote,
        crate::disk::TOOLCHAIN,
    );
    if !sel.problems.is_empty() {
        for p in &sel.problems {
            rep.diag(&disk::problem_diag(&shown, p));
        }
        return rep.finish(HD_FAILURE);
    }
    let mut added = sum.clone();
    for (key, hash) in sel.sums {
        added.entries.entry(key).or_insert(hash);
    }
    if added != sum {
        let path = sum_dir.join("hd.sum");
        if let Err(e) = std::fs::write(&path, added.text()) {
            return rep.fail(&format!("{}: {e}", path.display()));
        }
    }
    rep.finish(0)
}
