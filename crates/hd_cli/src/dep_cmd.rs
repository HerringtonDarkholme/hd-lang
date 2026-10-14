//! The dependency commands (Dependency Commands, `cli.dep.*`):
//! `hd add`, `hd update`, `hd remove` and `hd fetch`. Each computes its
//! whole result first, the edited `hd.toml` and the tidied `hd.sum`, and
//! writes last, so an error leaves both files as they were
//! (`cli.dep.unchanged-on-error`). None asks a question
//! (`cli.dep.no-question`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use std::fmt::Write as _;

use hd_deps::query::Query;
use hd_deps::target::TargetForm;
use hd_deps::{Cache, Edited, Mode, Remote, Source as _, Sum};
use hd_diag::{Code, Severity};
use hd_project::{HostPath, Manifest, Requirement, Version};

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
        let shown = hd_run::canonicalize(&root).unwrap_or_else(|_| root.clone());
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
            edited.filter(|e| hd_run::canonicalize(e.dir).ok() == hd_run::canonicalize(dir).ok());
        let text = match replaced {
            Some(e) => Ok(e.text.to_owned()),
            None => std::fs::read_to_string(&path).map_err(|e| e.to_string()),
        };
        let m = match text.and_then(|t| hd_project::parse_manifest(&t).map_err(|e| e.to_string())) {
            Ok(m) => m,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let file = disk::shown_from(shown, &hd_run::canonicalize(&path).unwrap_or(path));
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
/// error nothing is written (`cli.dep.unchanged-on-error`). `new_lines`
/// are the lines of the requirements that only `edited` holds.
fn commit(
    rep: &mut Reporter,
    target: &Target,
    edited: &str,
    new_lines: &[u32],
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
    for &line in new_lines {
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
        for &line in new_lines {
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

/// The error of a target, with its code, and the end of the command.
fn target_fail(rep: &mut Reporter, code: Option<Code>, message: &str) -> ExitCode {
    rep.diag(&Diag::error(code, message));
    rep.finish(HD_FAILURE)
}

/// `path` relative to `from`, both canonical, with `/` between its
/// segments (`cli.dep.target.path`).
fn relative(from: &Path, path: &Path) -> String {
    use std::path::Component;
    let a: Vec<Component<'_>> = from.components().collect();
    let b: Vec<Component<'_>> = path.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".to_owned(); a.len() - common];
    parts.extend(
        b[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

/// What a target's key requires: a local directory or a host path.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Names {
    Dir(PathBuf),
    Host(String),
}

/// One target of `hd add`, resolved: its key, the value its line writes,
/// and what that requires.
struct Plan {
    key: String,
    /// Whether the key came from the package's name, not `--name`.
    inferred: bool,
    /// The value's TOML text.
    value: String,
    names: Names,
    /// A fetched target's version.
    version: Option<Version>,
    /// Whether that version is the pseudo-version of an untagged package
    /// (`cli.dep.query.latest.note`).
    untagged: bool,
}

/// The `fetch-failed` message of a query other than an exact version,
/// which also suggests one (`cli.dep.query.offline`).
fn offline_hint((code, message): hd_deps::Failure, query: &Query, word: &str) -> hd_deps::Failure {
    if code == Some(Code::FetchFailed) && !matches!(query, Query::Exact(_)) {
        let (target, _) = hd_deps::target::split_target(word);
        let message = format!(
            "{message}; without the network, name an exact version that the cache holds, as `{target}@1.2.0`"
        );
        return (code, message);
    }
    (code, message)
}

/// The note of an untagged package (`cli.dep.query.latest.note`).
fn untagged_note(host: &HostPath) {
    let first = Version {
        major: 0,
        minor: 1,
        patch: 0,
        pre: Vec::new(),
    };
    let tag = hd_deps::version::tag_name(host, &first);
    eprintln!(
        "hd: note: {} has no version tag, so its requirement is the pseudo-version of its default branch's head; tag a release, as `git tag {tag}`, and push the tag",
        host.path
    );
}

/// A path target (`cli.dep.target.path`, `cli.dep.target.self`): the
/// directory, relative to the working directory, of a package with a
/// library (`cli.dep.no-library`).
fn path_plan(target: &Target, text: &str, name: Option<&str>) -> Result<Plan, hd_deps::Failure> {
    let invalid = |m: String| (Some(Code::InvalidRequirement), m);
    let cwd = std::env::current_dir().map_err(|e| (None, e.to_string()))?;
    let dir = hd_run::canonicalize(cwd.join(text))
        .map_err(|_| invalid(format!("the path target `{text}` names no directory")))?;
    if dir == target.shown {
        return Err(invalid(format!(
            "the path target `{text}` is this package's own directory, and a package cannot require itself"
        )));
    }
    let manifest = std::fs::read_to_string(dir.join("hd.toml"))
        .ok()
        .and_then(|t| hd_project::parse_manifest(&t).ok())
        .filter(|m| m.declares_package && !m.name.is_empty())
        .ok_or_else(|| {
            invalid(format!(
                "the directory `{text}` holds no package: it has no `hd.toml` with a `[package]` section"
            ))
        })?;
    let src = manifest
        .source_root
        .as_ref()
        .map_or("src", |(d, _)| d.trim_end_matches('/'));
    if !dir.join(src).join("lib.hd").is_file() {
        return Err(invalid(format!(
            "the package in `{text}` has no library (`{src}/lib.hd`), so no package can depend on it"
        )));
    }
    let (key, inferred) = match name {
        Some(n) => (n.to_owned(), false),
        None => (
            hd_deps::target::inferred_key(&manifest.name, &format!("`{text}`")).map_err(invalid)?,
            true,
        ),
    };
    Ok(Plan {
        key,
        inferred,
        value: hd_project::edit::path_value(&relative(&target.shown, &dir)),
        names: Names::Dir(dir),
        version: None,
        untagged: false,
    })
}

/// The key that the `hd.toml` of `version` of the package at `host`
/// gives (`cli.dep.key.inferred`), fetching it into the cache; reads only
/// that file of the tree (`cli.dep.no-code`).
fn fetched_key(
    remote: &mut Remote<'_>,
    host: &HostPath,
    version: &Version,
) -> Result<String, hd_deps::Failure> {
    let entry = remote.entry(host, version)?;
    let shown = format!("{}@{version}", host.path);
    let text = std::fs::read_to_string(entry.dir.join("hd.toml"))
        .map_err(|e| (None, format!("{}: {e}", entry.dir.display())))?;
    let manifest = hd_project::parse_manifest(&text).map_err(|e| {
        (
            Some(Code::InvalidRequirement),
            format!("{shown}: hd.toml: {e}"),
        )
    })?;
    hd_deps::target::inferred_key(&manifest.name, &shown)
        .map_err(|m| (Some(Code::InvalidRequirement), m))
}

/// A fetched target: the version its query selects, and its key from the
/// fetched `hd.toml` unless `--name` gives one.
fn host_plan(
    remote: &mut Remote<'_>,
    host: &HostPath,
    query: &Query,
    word: &str,
    name: Option<&str>,
) -> Result<Plan, hd_deps::Failure> {
    let resolved = remote
        .resolve(host, query)
        .map_err(|f| offline_hint(f, query, word))?;
    let (key, inferred) = match name {
        Some(n) => (n.to_owned(), false),
        None => (
            fetched_key(remote, host, &resolved.version)
                .map_err(|f| offline_hint(f, query, word))?,
            true,
        ),
    };
    Ok(Plan {
        key,
        inferred,
        value: hd_project::edit::string_value(&format!("{}@{}", host.path, resolved.version)),
        names: Names::Host(host.path.clone()),
        version: Some(resolved.version),
        untagged: resolved.untagged,
    })
}

/// What an existing requirement requires, read from the package at `root`.
fn names_of(root: &Path, r: &Requirement) -> Option<Names> {
    if let Some(dir) = &r.path {
        return hd_run::canonicalize(root.join(dir)).ok().map(Names::Dir);
    }
    if r.table {
        return None;
    }
    let path = r.text.rsplit_once('@').map_or(r.text.as_str(), |(p, _)| p);
    Some(Names::Host(path.to_owned()))
}

/// `hd add [--dev] [--name NAME] TARGET[@QUERY]...`
/// (`cli.dep.add.target`, `cli.dep.add.check`, `cli.dep.add.lower`,
/// `cli.dep.add.dev`, `cli.dep.add.other-table`, `cli.dep.add.several`,
/// `cli.dep.add.report*`, Targets, Version Queries, Dependency Keys).
#[allow(clippy::too_many_lines)]
pub(crate) fn add(args: &[OsString]) -> ExitCode {
    let (json, words) = match words(args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stdout(json);
    let mut dev = false;
    let mut name: Option<String> = None;
    let mut operands = Vec::new();
    let mut words = words.into_iter();
    while let Some(w) = words.next() {
        match w.as_str() {
            "--dev" => dev = true,
            "--name" => match words.next() {
                Some(n) => name = Some(n),
                None => return rep.command_fail("add", "`--name` takes the key NAME"),
            },
            option if option.starts_with("--name=") => {
                name = Some(option["--name=".len()..].to_owned());
            }
            option if option.starts_with('-') => {
                return rep.command_fail("add", &format!("`hd add` has no option `{option}`"));
            }
            _ => operands.push(w),
        }
    }
    if operands.is_empty() {
        return rep.command_fail(
            "add",
            "`hd add` takes one or more targets, as `hd add acme/json` or `hd add ../billing`",
        );
    }
    // `cli.dep.key.name-flag.one`.
    if name.is_some() && operands.len() > 1 {
        return rep.command_fail(
            "add",
            "`--name` gives the key of one target, and this command has several; add them one at a time",
        );
    }
    let target = match Target::open("add") {
        Ok(t) => t,
        Err(e) => return rep.command_fail("add", &e),
    };
    // Every target and the key are checked before anything is fetched
    // (`cli.dep.add.check`).
    if let Some(why) = name.as_deref().and_then(hd_project::key_problem) {
        return target_fail(&mut rep, Some(Code::InvalidRequirement), &why);
    }
    let mut parsed = Vec::new();
    for word in &operands {
        match hd_deps::target::parse_target(word) {
            Ok(t) => parsed.push((word.as_str(), t)),
            Err(why) => return target_fail(&mut rep, Some(Code::InvalidRequirement), &why),
        }
    }
    let (table, other) = if dev {
        ("dev-dependencies", "dependencies")
    } else {
        ("dependencies", "dev-dependencies")
    };
    let in_other = |key: &str| {
        let wanted = key.replace('-', "_");
        target
            .requirements(other)
            .iter()
            .find(|r| r.name() == wanted)
            .map(|r| r.key.clone())
    };
    // With the key from `--name`, the other table is checked before any
    // fetch (`cli.dep.add.other-table`).
    if let Some(n) = &name
        && let Some(key) = in_other(n)
    {
        return rep.command_fail(
            "add",
            &format!("{key} is in [{other}]; run `hd remove {key}` first"),
        );
    }
    remote!(remote);
    let mut plans: Vec<Plan> = Vec::new();
    for (word, t) in &parsed {
        let plan = match &t.form {
            TargetForm::Path(text) => path_plan(&target, text, name.as_deref()),
            TargetForm::Host(host) => {
                let query = match hd_deps::query::parse_query(t.query.as_deref()) {
                    Ok(q) => q,
                    Err(why) => return target_fail(&mut rep, Some(Code::InvalidRequirement), &why),
                };
                host_plan(&mut remote, host, &query, word, name.as_deref())
            }
        };
        match plan {
            Ok(p) => plans.push(p),
            Err((code, message)) => return target_fail(&mut rep, code, &message),
        }
    }
    // Dependency Keys: the other table, the key's current requirement, and
    // keys that two targets share.
    let mut old: Vec<Option<Requirement>> = Vec::new();
    for (i, plan) in plans.iter_mut().enumerate() {
        if let Some(key) = in_other(&plan.key) {
            let hint = if plan.inferred {
                format!(
                    ", or choose another key with `hd add --name NAME {}`",
                    operands[i]
                )
            } else {
                String::new()
            };
            return rep.command_fail(
                "add",
                &format!("{key} is in [{other}]; run `hd remove {key}` first{hint}"),
            );
        }
        let wanted = plan.key.replace('-', "_");
        if let Some(r) = target
            .requirements(table)
            .iter()
            .find(|r| r.name() == wanted)
        {
            if names_of(&target.root, r).as_ref() != Some(&plan.names) {
                return target_fail(
                    &mut rep,
                    Some(Code::InvalidRequirement),
                    &format!(
                        "the key `{}` already requires {}; choose another key with `hd add --name NAME {}`, or run `hd remove {}` first",
                        r.key,
                        r.path
                            .as_deref()
                            .map_or_else(|| r.text.clone(), |p| format!("the directory `{p}`")),
                        operands[i],
                        r.key
                    ),
                );
            }
            // `cli.dep.key.same`: the key keeps its spelling.
            plan.key.clone_from(&r.key);
            old.push(Some(r.clone()));
        } else {
            old.push(None);
        }
    }
    for (i, plan) in plans.iter().enumerate() {
        let wanted = plan.key.replace('-', "_");
        if let Some(j) = plans[..i]
            .iter()
            .position(|p| p.key.replace('-', "_") == wanted)
        {
            return target_fail(
                &mut rep,
                Some(Code::InvalidRequirement),
                &format!(
                    "the targets `{}` and `{}` both take the key `{}`; add one of them with `hd add --name NAME TARGET`",
                    operands[j], operands[i], plan.key
                ),
            );
        }
    }
    let mut edited = target.text.clone();
    for plan in &plans {
        match hd_project::edit::set_value(&edited, table, &plan.key, &plan.value) {
            Some(next) => edited = next,
            None => return rep.command_fail("add", &target.cannot_edit(table)),
        }
    }
    let new_lines: Vec<u32> = hd_project::parse_manifest(&edited)
        .ok()
        .map(|m| {
            let reqs = if dev {
                m.dev_dependencies
            } else {
                m.dependencies
            };
            reqs.into_iter()
                .filter(|r| target.find(&r.key).is_none())
                .map(|r| r.line)
                .collect()
        })
        .unwrap_or_default();
    if let Err(code) = commit(&mut rep, &target, &edited, &new_lines, &mut remote) {
        return code;
    }
    if !json {
        for (plan, old) in plans.iter().zip(&old) {
            let mut line = format!("{} = {}", plan.key, plan.value);
            if let Some(v) = &plan.version
                && let Some(c) = hd_deps::version::pseudo_commit(v)
            {
                let _ = write!(line, " # commit {} has no version tag", c.hash);
            }
            println!("{line}");
            if let (Some(old), Some(v), Names::Host(path)) = (old, &plan.version, &plan.names)
                && let Some(was) = hd_deps::lowered(old, path, v)
            {
                println!("lowered {} {was} -> {v}", plan.key);
            }
        }
    }
    for ((_, t), plan) in parsed.iter().zip(&plans) {
        if plan.untagged
            && let TargetForm::Host(host) = &t.form
        {
            untagged_note(host);
        }
    }
    rep.finish(0)
}

/// `hd update NAME@QUERY` (`cli.dep.update.query`,
/// `cli.dep.update.query.path`): sets NAME's requirement to the version
/// QUERY selects on its host path, raising or lowering it.
fn update_query(rep: &mut Reporter, json: bool, name: &str, query: &str) -> ExitCode {
    let target = match Target::open("update") {
        Ok(t) => t,
        Err(e) => return rep.command_fail("update", &e),
    };
    let Some((table, r)) = target.find(name) else {
        return rep.command_fail("update", &no_key(name));
    };
    let invalid = Some(Code::InvalidRequirement);
    if r.path.is_some() || r.table {
        return target_fail(
            rep,
            invalid,
            &format!(
                "`{name}` holds a path requirement, which uses its directory as it is and takes no version query"
            ),
        );
    }
    let Some((path, current)) = r.host() else {
        return target_fail(
            rep,
            invalid,
            &format!(
                "`{name}` holds `{}`, which is no requirement PATH@VERSION",
                r.text
            ),
        );
    };
    let host = match hd_project::host_path(path) {
        Ok(h) => h,
        Err(why) => return target_fail(rep, invalid, &why),
    };
    let query = match hd_deps::query::parse_query(Some(query)) {
        Ok(q) => q,
        Err(why) => return target_fail(rep, invalid, &why),
    };
    remote!(remote);
    let resolved = match remote.resolve(&host, &query) {
        Ok(r) => r,
        Err(f) => {
            let (code, message) = offline_hint(f, &query, name);
            return target_fail(rep, code, &message);
        }
    };
    let value = format!("{path}@{}", resolved.version);
    let Some(edited) = hd_project::edit::set_key(&target.text, table, &r.key, &value) else {
        return rep.command_fail("update", &target.cannot_edit(table));
    };
    let key = r.key.clone();
    if let Err(code) = commit(rep, &target, &edited, &[], &mut remote) {
        return code;
    }
    if !json {
        let new = &resolved.version;
        if *new < current {
            println!("lowered {key} {current} -> {new}");
        } else if *new > current {
            println!("{key}: {current} -> {new}");
        }
    }
    if resolved.untagged {
        untagged_note(&host);
    }
    rep.finish(0)
}

/// `hd update [NAME[@QUERY]]` (`cli.dep.update`, `cli.dep.update.release`,
/// `cli.dep.update.network`, `cli.dep.update.dev`).
pub(crate) fn update(args: &[OsString]) -> ExitCode {
    let (json, words) = match words(args) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stdout(json);
    let name = match words.as_slice() {
        [] => None,
        [n] if !n.starts_with('-') => {
            if let Some((name, query)) = n.split_once('@') {
                return update_query(&mut rep, json, name, query);
            }
            Some(n.clone())
        }
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
    if let Err(code) = commit(&mut rep, &target, &edited, &[], &mut remote) {
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
    match commit(&mut rep, &target, &edited, &[], &mut remote) {
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
    let shown = hd_run::canonicalize(&shown).unwrap_or(shown);
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::relative;

    #[test]
    fn path_targets_are_written_relative_to_the_manifest() {
        let root = Path::new("/w/shop");
        assert_eq!(
            relative(root, Path::new("/w/shop/money-utils")),
            "money-utils"
        );
        assert_eq!(relative(root, Path::new("/w/billing")), "../billing");
        assert_eq!(relative(root, Path::new("/w/shop/vendor/a")), "vendor/a");
        assert_eq!(relative(root, Path::new("/x/y")), "../../x/y");
        assert_eq!(relative(root, root), ".");
    }
}
