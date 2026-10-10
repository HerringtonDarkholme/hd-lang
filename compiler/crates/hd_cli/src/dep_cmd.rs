//! The dependency commands (Dependency Commands, `cli.dep.*`):
//! `hd add`, `hd update`, `hd remove` and `hd fetch`. Each computes its
//! whole result first, the edited `hd.toml` and the tidied `hd.sum`, and
//! writes last, so an error leaves both files as they were
//! (`cli.dep.unchanged-on-error`). None asks a question
//! (`cli.dep.no-question`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hd_deps::{Cache, Edited, Mode, Remote, Sum};
use hd_diag::{Code, Severity};
use hd_project::{Manifest, Requirement, Version};

use crate::report::{Diag, Reporter, format_flag};
use crate::{HD_FAILURE, disk, fail};

/// The tables of requirements, in the order a manifest lists them.
const TABLES: [&str; 2] = ["dependencies", "dev-dependencies"];

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

/// The package a command edits, with its manifest and where selection
/// starts.
struct Target {
    command: &'static str,
    root: PathBuf,
    /// The canonical package directory, which file names in messages are
    /// shown from.
    shown: PathBuf,
    /// The directory of the `hd.sum` the package uses: its own, or its
    /// workspace's (`module.workspace.sum`).
    sum_dir: PathBuf,
    /// The packages selection starts at: the package, and the other
    /// members of its workspace.
    members: Vec<PathBuf>,
    text: String,
    manifest: Manifest,
}

/// The error of a NAME that is no key of either table.
fn no_key(name: &str) -> String {
    format!("`{name}` is no key of `[dependencies]` or `[dev-dependencies]` in hd.toml")
}

impl Target {
    fn open(command: &'static str) -> Result<Target, String> {
        let root = package(command)?;
        let path = root.join("hd.toml");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let manifest = hd_project::parse_manifest(&text).map_err(|e| format!("hd.toml: {e}"))?;
        let (sum_dir, members) = disk::selection_start(&root);
        let shown = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
        Ok(Target {
            command,
            root,
            shown,
            sum_dir,
            members,
            text,
            manifest,
        })
    }

    fn requirements(&self, table: &str) -> &[Requirement] {
        if table == "dev-dependencies" {
            &self.manifest.dev_dependencies
        } else {
            &self.manifest.dependencies
        }
    }

    /// The table and requirement of the key `name`.
    fn find(&self, name: &str) -> Option<(&'static str, &Requirement)> {
        TABLES.into_iter().find_map(|table| {
            self.requirements(table)
                .iter()
                .find(|r| r.key == name)
                .map(|r| (table, r))
        })
    }

    fn cannot_edit(&self, table: &str) -> String {
        format!(
            "hd.toml writes `[{table}]` in a form `hd {}` does not edit, such as `[{table}.NAME]` or an inline table; change it by hand",
            self.command
        )
    }
}

/// The text of `hd.sum` to write, if it differs from what the file holds.
/// With no file and no entry there is nothing to write (`cli.dep.path`).
fn sum_to_write(old: Option<&str>, sum: &Sum) -> Option<String> {
    let new = sum.text();
    match old {
        Some(old) if old == new => None,
        None if new.is_empty() => None,
        _ => Some(new),
    }
}

/// Writes every file, each through a temporary file beside it, so a file
/// is never half written and a failure to stage one changes none.
fn write_files(files: &[(PathBuf, String)]) -> Result<(), String> {
    let mut staged: Vec<(PathBuf, &Path)> = Vec::new();
    for (path, text) in files {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
        if let Err(e) = std::fs::write(&tmp, text) {
            for (done, _) in &staged {
                let _ = std::fs::remove_file(done);
            }
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("{}: {e}", path.display()));
        }
        staged.push((tmp, path));
    }
    for (tmp, path) in staged {
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// The `hd.sum` in `sum_dir`, or the reported error of why it cannot be
/// read.
fn read_sum(rep: &mut Reporter, shown: &Path, sum_dir: &Path) -> Result<Sum, ExitCode> {
    disk::read_sum(sum_dir).map_err(|(line, message)| {
        let file = disk::shown_from(shown, &sum_dir.join("hd.sum"));
        rep.diag(&Diag::error(None, &message).at(&file, Some(line as usize)));
        rep.finish(HD_FAILURE)
    })
}

/// Each member's requirements that break a rule of the manifest alone
/// (`cli.dep.invalid`), or whose path names no package
/// (`module.path-dep.no-package`); selection leaves them out. `edited` is
/// the manifest a command is about to write, read in place of its file.
fn invalid_diags(
    shown: &Path,
    members: &[PathBuf],
    edited: Option<Edited<'_>>,
) -> Result<Vec<Diag>, String> {
    let mut diags = Vec::new();
    for dir in members {
        let path = dir.join("hd.toml");
        let replaced =
            edited.filter(|e| std::fs::canonicalize(e.dir).ok() == std::fs::canonicalize(dir).ok());
        let text = match replaced {
            Some(e) => Ok(e.text.to_owned()),
            None => std::fs::read_to_string(&path).map_err(|e| e.to_string()),
        };
        let m = match text.and_then(|t| hd_project::parse_manifest(&t).map_err(|e| e.to_string())) {
            Ok(m) => m,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let file = disk::shown_from(shown, &std::fs::canonicalize(&path).unwrap_or(path));
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
                Err(e) => return Err(e),
            }
        }
    }
    Ok(diags)
}

/// Drops the position of each diagnostic at `line` of `file`: that line of
/// the manifest to write is not in the manifest on disk.
fn unplace(diags: &mut [Diag], file: &str, line: u32) {
    for d in diags {
        if d.file.as_deref() == Some(file) && d.line == Some(line as usize) {
            d.file = None;
            d.line = None;
            d.column = None;
        }
    }
}

/// The part of a command after it has changed the manifest
/// (`cli.dep.select`, `cli.dep.tidy`): selects for the package with
/// `edited` as its `hd.toml`, fetches each version the cache lacks, then
/// writes the manifest and an `hd.sum` that holds one line for each
/// selected version, one for each manifest selection read, and no other
/// entry. A version's existing line is kept (`cli.sum.keep`). On any
/// error nothing is written (`cli.dep.unchanged-on-error`). `new_line` is
/// the line of a requirement that only `edited` holds.
fn commit(
    rep: &mut Reporter,
    target: &Target,
    edited: &str,
    new_line: Option<u32>,
    remote: &mut Remote<'_>,
) -> Result<(), ExitCode> {
    let edit = Edited {
        dir: &target.root,
        text: edited,
    };
    let own = disk::shown_from(&target.shown, &target.shown.join("hd.toml"));
    let mut diags = match invalid_diags(&target.shown, &target.members, Some(edit)) {
        Ok(d) => d,
        Err(e) => return Err(rep.command_fail(target.command, &e)),
    };
    if let Some(line) = new_line {
        unplace(&mut diags, &own, line);
    }
    if rep.diags(&diags) {
        return Err(rep.finish(HD_FAILURE));
    }
    let sum = read_sum(rep, &target.shown, &target.sum_dir)?;
    let sel = hd_deps::select_edited(
        &target.members,
        Some(edit),
        &sum,
        Mode::Record,
        remote,
        disk::TOOLCHAIN,
    );
    if !sel.problems.is_empty() {
        let mut diags: Vec<Diag> = sel
            .problems
            .iter()
            .map(|p| disk::problem_diag(&target.shown, p))
            .collect();
        if let Some(line) = new_line {
            unplace(&mut diags, &own, line);
        }
        rep.diags(&diags);
        return Err(rep.finish(HD_FAILURE));
    }
    let mut files = Vec::new();
    if edited != target.text {
        files.push((target.root.join("hd.toml"), edited.to_owned()));
    }
    let sum_path = target.sum_dir.join("hd.sum");
    let old = std::fs::read_to_string(&sum_path).ok();
    if let Some(text) = sum_to_write(old.as_deref(), &Sum { entries: sel.sums }) {
        files.push((sum_path, text));
    }
    write_files(&files).map_err(|e| rep.command_fail(target.command, &e))
}

/// A new cache and git for one command, with a line on standard error
/// before each fetch.
macro_rules! remote {
    ($remote:ident) => {
        let mut progress = |key: &str| eprintln!("hd: fetching {key}");
        let mut $remote = Remote::new(Cache::new(crate::cache_root()), &mut progress);
    };
}

/// `hd add [--dev] NAME PATH@VERSION` (`cli.dep.add`, `cli.dep.add.check`,
/// `cli.dep.add.lower`, `cli.dep.add.dev`, `cli.dep.add.other-table`).
pub(crate) fn add(args: &[OsString]) -> ExitCode {
    let (json, words) = match words(args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stdout(json);
    let mut dev = false;
    let mut operands = Vec::new();
    for w in words {
        match w.as_str() {
            "--dev" => dev = true,
            option if option.starts_with('-') => {
                return rep.command_fail("add", &format!("`hd add` has no option `{option}`"));
            }
            _ => operands.push(w),
        }
    }
    let [name, requirement] = operands.as_slice() else {
        return rep.command_fail("add", "`hd add` takes NAME and PATH@VERSION");
    };
    let target = match Target::open("add") {
        Ok(t) => t,
        Err(e) => return rep.command_fail("add", &e),
    };
    // Both are checked before anything is fetched (`cli.dep.add.check`).
    if let Some(why) = hd_project::requirement_problem(name, requirement) {
        rep.diag(&Diag::error(Some(Code::InvalidRequirement), &why));
        return rep.finish(HD_FAILURE);
    }
    let (table, other) = if dev {
        ("dev-dependencies", "dependencies")
    } else {
        ("dependencies", "dev-dependencies")
    };
    if target.requirements(other).iter().any(|r| r.key == *name) {
        return rep.command_fail(
            "add",
            &format!("{name} is in [{other}]; run `hd remove {name}` first"),
        );
    }
    let lowered = requirement.rsplit_once('@').and_then(|(path, version)| {
        let old = target.requirements(table).iter().find(|r| r.key == *name)?;
        hd_deps::lowered(old, path, &Version::parse(version)?)
    });
    let Some(edited) = hd_project::edit::set_key(&target.text, table, name, requirement) else {
        return rep.command_fail("add", &target.cannot_edit(table));
    };
    let new_line = hd_project::parse_manifest(&edited).ok().and_then(|m| {
        let reqs = if dev {
            m.dev_dependencies
        } else {
            m.dependencies
        };
        reqs.into_iter().find(|r| r.key == *name).map(|r| r.line)
    });
    remote!(remote);
    if let Err(code) = commit(&mut rep, &target, &edited, new_line, &mut remote) {
        return code;
    }
    if let Some(old) = lowered
        && !json
    {
        let version = requirement.rsplit_once('@').map_or("", |(_, v)| v);
        println!("lowered {name} {old} -> {version}");
    }
    rep.finish(0)
}

/// `hd update [NAME]` (`cli.dep.update`, `cli.dep.update.release`,
/// `cli.dep.update.network`, `cli.dep.update.dev`).
pub(crate) fn update(args: &[OsString]) -> ExitCode {
    let (json, words) = match words(args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stdout(json);
    let name = match words.as_slice() {
        [] => None,
        [n] if !n.starts_with('-') => Some(n.clone()),
        [n, ..] if n.starts_with('-') => {
            return rep.command_fail("update", &format!("`hd update` has no option `{n}`"));
        }
        _ => return rep.command_fail("update", "`hd update` takes at most one NAME"),
    };
    let target = match Target::open("update") {
        Ok(t) => t,
        Err(e) => return rep.command_fail("update", &e),
    };
    if let Some(n) = &name
        && target.find(n).is_none()
    {
        return rep.command_fail("update", &no_key(n));
    }
    remote!(remote);
    // Each requirement that names a version, in manifest order; a path
    // requirement or an invalid one has no tag to move to, and the check
    // after the edit reports an invalid one.
    let mut edited = target.text.clone();
    let mut moved: Vec<(String, String, String)> = Vec::new();
    for table in TABLES {
        for r in target.requirements(table) {
            if name.as_ref().is_some_and(|n| *n != r.key) {
                continue;
            }
            let Some((path, current)) = r.host() else {
                continue;
            };
            let Ok(host) = hd_project::host_path(path) else {
                continue;
            };
            let versions = match remote.versions(&host) {
                Ok(v) => v,
                Err((code, message)) => {
                    rep.diag(&Diag::error(code, &message));
                    return rep.finish(HD_FAILURE);
                }
            };
            let Some(newest) = hd_deps::newest_release(&versions, &current) else {
                continue;
            };
            let value = format!("{path}@{newest}");
            let Some(next) = hd_project::edit::set_key(&edited, table, &r.key, &value) else {
                return rep.command_fail("update", &target.cannot_edit(table));
            };
            edited = next;
            moved.push((r.key.clone(), current.to_string(), newest.to_string()));
        }
    }
    if let Err(code) = commit(&mut rep, &target, &edited, None, &mut remote) {
        return code;
    }
    if !json {
        for (key, from, to) in moved {
            println!("{key}: {from} -> {to}");
        }
    }
    rep.finish(0)
}

/// `hd remove NAME` (`cli.dep.remove`, `cli.dep.edit`,
/// `cli.dep.edit.empty-table`, `cli.dep.tidy`,
/// `cli.dep.unchanged-on-error`).
pub(crate) fn remove(args: &[OsString]) -> ExitCode {
    let (json, words) = match words(args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stdout(json);
    let name = match words.as_slice() {
        [n] if !n.starts_with('-') => n.clone(),
        [n, ..] if n.starts_with('-') => {
            return rep.command_fail("remove", &format!("`hd remove` has no option `{n}`"));
        }
        _ => return rep.command_fail("remove", "`hd remove` takes one NAME"),
    };
    let target = match Target::open("remove") {
        Ok(t) => t,
        Err(e) => return rep.command_fail("remove", &e),
    };
    let Some((table, _)) = target.find(&name) else {
        return rep.command_fail("remove", &no_key(&name));
    };
    let Some(edited) = hd_project::edit::remove_key(&target.text, table, &name) else {
        return rep.command_fail("remove", &target.cannot_edit(table));
    };
    remote!(remote);
    match commit(&mut rep, &target, &edited, None, &mut remote) {
        Ok(()) => rep.finish(0),
        Err(code) => code,
    }
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
    match invalid_diags(&shown, &members, None) {
        Ok(diags) => {
            if rep.diags(&diags) {
                return rep.finish(HD_FAILURE);
            }
        }
        Err(e) => return rep.fail(&e),
    }
    let sum = match read_sum(&mut rep, &shown, &sum_dir) {
        Ok(s) => s,
        Err(code) => return code,
    };
    remote!(remote);
    let sel = hd_deps::select(&members, &sum, Mode::Record, &mut remote, disk::TOOLCHAIN);
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
        if let Err(e) = write_files(&[(path, added.text())]) {
            return rep.fail(&e);
        }
    }
    rep.finish(0)
}
