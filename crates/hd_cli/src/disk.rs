//! The disk `SourceSet` and program discovery: the package root, its
//! name from `hd.toml`, and the entry module.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hd_diag::{Code, Severity};
use hd_driver::{BuildProfile, Dependency, Packages};
use hd_project::{Grant, Manifest, SourceEntry, SourceSet, module_below, parse_manifest};

use crate::report::{Diag, Edit, Fix};

/// This toolchain's version, which a manifest's `[package] hd` minimum is
/// compared with (`module.toolchain.graph-minimum`).
pub(crate) const TOOLCHAIN: &str = env!("CARGO_PKG_VERSION");

/// Every `.hd` file under a root, read on demand. Discovery knows the
/// source root as `src`; a package whose `[source] root` names another
/// directory (`module.manifest.source-root`) has its files listed under
/// `src/`, and `display` and `disk_path` name them as they are on disk.
#[derive(Clone)]
pub struct DiskSources {
    root: PathBuf,
    files: Vec<SourceEntry>,
    /// The source root on disk, when it is not `src`.
    src: Option<String>,
    /// Files no disk holds, such as the program of a doc test, by path.
    synthetic: Vec<(String, Arc<[u8]>)>,
}

impl DiskSources {
    /// Keeps the files whose package-relative path `keep` accepts.
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.files.retain(|e| keep(&e.path));
    }

    /// Adds a file no disk holds, listed at `path` (a doc test's program,
    /// `crates/hd_tir/DESIGN.md` §4.13.9).
    pub fn add_synthetic(&mut self, path: &str, text: &str) {
        self.files.push(SourceEntry {
            path: path.to_owned(),
            size: text.len() as u64,
        });
        self.files.sort_by(|a, b| a.path.cmp(&b.path));
        self.synthetic
            .push((path.to_owned(), Arc::from(text.as_bytes())));
    }

    /// Reads the listed file at `path` as `text` instead of from disk, as
    /// `hd fix` checks an edit before it writes it (`cli.fix.safe`).
    pub fn set_text(&mut self, path: &str, text: &str) {
        if let Some(e) = self.files.iter_mut().find(|e| e.path == path) {
            e.size = text.len() as u64;
        }
        self.synthetic.retain(|(p, _)| p != path);
        self.synthetic
            .push((path.to_owned(), Arc::from(text.as_bytes())));
    }

    /// Removes a file that `add_synthetic` added, such as a REPL input
    /// that did not join its session.
    pub fn remove_synthetic(&mut self, path: &str) {
        self.files.retain(|e| e.path != path);
        self.synthetic.retain(|(p, _)| p != path);
    }

    /// A listed path as it is on disk, relative to the package directory.
    pub fn display(&self, path: &str) -> String {
        match (&self.src, path.strip_prefix("src/")) {
            (Some(dir), Some(rest)) => format!("{dir}/{rest}"),
            _ => path.to_owned(),
        }
    }

    /// A path relative to the package directory as discovery lists it.
    pub fn listed(&self, path: &str) -> String {
        match &self.src {
            Some(dir) => path
                .strip_prefix(dir.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
                .map_or_else(|| path.to_owned(), |rest| format!("src/{rest}")),
            None => path.to_owned(),
        }
    }

    /// Where a listed path is on disk.
    pub fn disk_path(&self, path: &str) -> PathBuf {
        self.root.join(self.display(path))
    }
}

impl SourceSet for DiskSources {
    fn list(&self) -> Vec<SourceEntry> {
        self.files.clone()
    }
    fn read(&self, path: &str) -> Option<Arc<[u8]>> {
        if let Some((_, text)) = self.synthetic.iter().find(|(p, _)| p == path) {
            return Some(text.clone());
        }
        read_regular(&self.disk_path(path)).map(Arc::from)
    }
}

/// What a directory entry is for a walk: a directory, a regular file, or
/// neither. A symbolic link is judged by its target, through `stat`, which
/// never opens it. FIFOs, sockets and devices are neither, so a walk never
/// opens one: opening a FIFO with no writer blocks forever.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Dir,
    File,
    Other,
}

pub(crate) fn kind_of(entry: &std::fs::DirEntry) -> Kind {
    let Ok(t) = entry.file_type() else {
        return Kind::Other;
    };
    let t = if t.is_symlink() {
        match std::fs::metadata(entry.path()) {
            Ok(m) => m.file_type(),
            Err(_) => return Kind::Other,
        }
    } else {
        t
    };
    if t.is_dir() {
        Kind::Dir
    } else if t.is_file() {
        Kind::File
    } else {
        Kind::Other
    }
}

/// The bytes of `path` if it is a regular file (after links), else `None`
/// without opening it.
pub(crate) fn read_regular(path: &Path) -> Option<Vec<u8>> {
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return None;
    }
    std::fs::read(path).ok()
}

/// A path relative to the package directory at `root`, as discovery lists
/// it: under `src/` when it lies in the package's source root.
pub fn listed_path(root: &Path, rel: &str) -> String {
    let src = source_dir(root);
    match rel
        .strip_prefix(src.as_str())
        .and_then(|r| r.strip_prefix('/'))
    {
        Some(rest) if src != "src" => format!("src/{rest}"),
        _ => rel.to_owned(),
    }
}

/// A package's source root on disk: its `[source] root`, or `src`
/// (`module.manifest.source-root`).
pub fn source_dir(root: &Path) -> String {
    manifest_of(root)
        .ok()
        .flatten()
        .and_then(|m| m.source_root)
        .map_or_else(
            || "src".to_owned(),
            |(dir, _)| dir.trim_end_matches('/').to_owned(),
        )
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<SourceEntry>) -> Result<(), String> {
    let real = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    walk_below(root, dir, out, &mut vec![real])
}

/// Lists the sources below `dir`. `open` holds the real paths of the
/// directories being walked, outermost first. A linked directory at or
/// above one of them (`src/loop -> ..`) would be walked again forever, so
/// it is skipped; any other linked directory is walked like a real one.
fn walk_below(
    root: &Path,
    dir: &Path,
    out: &mut Vec<SourceEntry>,
    open: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<(PathBuf, Kind)> =
        entries.flatten().map(|e| (e.path(), kind_of(&e))).collect();
    paths.sort_by(|a, b| a.0.cmp(&b.0));
    for (p, kind) in paths {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.starts_with('.') || name == "build" {
            continue;
        }
        if kind == Kind::Dir {
            let Ok(real) = std::fs::canonicalize(&p) else {
                continue;
            };
            if open.iter().any(|d| d.starts_with(&real)) {
                continue;
            }
            open.push(real);
            walk_below(root, &p, out, open)?;
            open.pop();
        } else if kind == Kind::File && p.extension().is_some_and(|x| x == "hd") {
            let rel = p
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let size = std::fs::metadata(&p).map_or(0, |m| m.len());
            out.push(SourceEntry { path: rel, size });
        }
    }
    Ok(())
}

/// A package's requirements: each dependency name and its package index,
/// 0 for the root and `i + 1` for `Program::deps[i]`.
pub type Requires = Vec<(String, u16)>;

pub struct Program {
    pub sources: DiskSources,
    pub package: String,
    /// The entry module below the package, as `main`.
    pub entry: String,
    /// The root's requirements, as `Packages::requires` indexes them.
    pub requires: Requires,
    /// The root's dev requirements, in the same form.
    pub dev_requires: Requires,
    /// The packages its path requirements reach.
    pub deps: Vec<DiskPackage>,
    /// What its manifest breaks (`crates/hd_cli/DESIGN.md` §7.1 step 1); any error stops
    /// the command before compiling.
    pub problems: Vec<Diag>,
    /// A single-file program's FILE as the command line wrote it
    /// (`cli.json.diagnostic.file`).
    pub as_written: Option<String>,
    /// Whether it is a single-file program, alone without an `hd.toml`
    /// (`trait.typeid.name.single-file`).
    pub single_file: bool,
    /// The `[capabilities]` table of its package, the valid keys
    /// (`cli.cap.source.package`); empty outside a package.
    pub capabilities: Vec<(String, Grant)>,
    /// Its package's directory, which the table's path entries are
    /// relative to; `None` outside a package.
    pub package_dir: Option<PathBuf>,
    /// Its executables' entry modules, by path below the source root.
    pub entries: Vec<String>,
}

/// A package that a path requirement reaches.
pub struct DiskPackage {
    pub name: String,
    /// Its identity, as `hd_deps::Package::identity`.
    pub identity: String,
    pub sources: DiskSources,
    pub requires: Requires,
    /// Selected for a version requirement, rather than a local path.
    pub fetched: bool,
}

impl Program {
    /// Narrows the sources to one program's (`hd_project::in_program`).
    pub fn only_program(&mut self, entry_file: &str) {
        self.sources
            .retain(|path| hd_project::in_program(entry_file, path));
    }

    /// The packages around the root, as the driver takes them.
    pub fn packages(&self) -> Packages<'_> {
        Packages {
            instruction_origins: false,
            checked: false,
            own_passes: false,
            requires: self.requires.clone(),
            deps: self
                .deps
                .iter()
                .map(|d| Dependency {
                    name: d.name.clone(),
                    identity: d.identity.clone(),
                    sources: &d.sources,
                    requires: d.requires.clone(),
                    fetched: d.fetched,
                })
                .collect(),
            entries: self.entries.clone(),
            dev_requires: self.dev_requires.clone(),
            profile: None,
            single_file: self.single_file,
            build_profile: BuildProfile::Checked,
            target: hd_driver::EngineTarget::default(),
            level: hd_driver::OptLevel::Dev,
            doc_scopes: false,
            check_roots: None,
            session: None,
            inputs_from: None,
            failed_inputs: &[],
            body_fuel: None,
        }
    }
}

/// A package directory's manifest, if it has one.
fn manifest_of(dir: &Path) -> Result<Option<Manifest>, String> {
    match std::fs::read_to_string(dir.join("hd.toml")) {
        Ok(text) => parse_manifest(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}", dir.join("hd.toml").display())),
        Err(_) => Ok(None),
    }
}

/// Where selection starts for the package at `root`, and the directory of
/// its `hd.sum`: the package and the other members of its workspace, with
/// the workspace root (`module.workspace.selection`,
/// `module.workspace.sum`), or the package alone (`module.sum.file`).
pub fn selection_start(root: &Path) -> (PathBuf, Vec<PathBuf>) {
    let Some((ws, members)) = enclosing_workspace(root) else {
        return (root.to_path_buf(), vec![root.to_path_buf()]);
    };
    let same = |a: &Path, b: &Path| {
        hd_run::canonicalize(a)
            .ok()
            .is_some_and(|a| hd_run::canonicalize(b).ok() == Some(a))
    };
    let mut all = vec![root.to_path_buf()];
    all.extend(members.into_iter().filter(|m| !same(m, root)));
    (ws, all)
}

/// The `hd.sum` in `dir`, empty when it has none, or the line and message
/// of why it cannot be read.
pub fn read_sum(dir: &Path) -> Result<hd_deps::Sum, (u32, String)> {
    match std::fs::read_to_string(dir.join("hd.sum")) {
        Ok(text) => hd_deps::Sum::parse(&text).map_err(|e| (e.line, e.message)),
        Err(_) => Ok(hd_deps::Sum::default()),
    }
}

/// `path` as shown from the directory `from`: relative to it when both
/// are below one directory, else as it is.
pub fn shown_from(from: &Path, path: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = path.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return path.display().to_string();
    }
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// A dependency problem as a diagnostic at its line of `hd.toml`, shown
/// from the directory `from`.
pub fn problem_diag(from: &Path, p: &hd_deps::Problem) -> Diag {
    let file = shown_from(from, &p.place.dir.join("hd.toml"));
    Diag::error(p.code, &p.message).at(&file, Some(p.place.line as usize))
}

/// The packages the root's requirements reach, selected and fetched
/// (`cli.dep.implicit-fetch`), in the order they are first reached, and
/// the root's own requirements and dev requirements, with the problems of
/// the selection at their lines. Only the root's `[dev-dependencies]` are
/// linked (`module.select.dev-dependencies`). One directory or version is
/// one package, so a cycle stays finite, and the build reports it
/// (`module.cycle.package`). It never writes `hd.sum`
/// (`cli.dep.no-sum-write`).
fn dependencies(root: &Path) -> Result<Dependencies, String> {
    if manifest_of(root)?.is_none() {
        return Ok(Dependencies::default());
    }
    let canonical = hd_run::canonicalize(root).map_err(|e| e.to_string())?;
    let (sum_dir, members) = selection_start(root);
    let mut problems = Vec::new();
    let sum = read_sum(&sum_dir).unwrap_or_else(|(line, message)| {
        let file = shown_from(&canonical, &sum_dir.join("hd.sum"));
        problems.push((
            None,
            Diag::error(None, &message).at(&file, Some(line as usize)),
        ));
        hd_deps::Sum::default()
    });
    let mut progress = |key: &str| eprintln!("hd: fetching {key}");
    let mut remote = hd_deps::Remote::new(hd_deps::Cache::new(crate::cache_root()), &mut progress);
    let sel = hd_deps::select(
        &members,
        &sum,
        hd_deps::Mode::Verify,
        &mut remote,
        TOOLCHAIN,
    );
    for p in &sel.problems {
        let line = (p.place.dir == canonical).then_some(p.place.line);
        problems.push((line, problem_diag(&canonical, p)));
    }
    let Some(first) = sel.packages.first().filter(|p| p.dir == canonical) else {
        return Ok(Dependencies {
            problems,
            ..Dependencies::default()
        });
    };
    // The root's part of the graph, the root first.
    let mut index: hd_base::HashMap<usize, u16> = [(0, 0)].into_iter().collect();
    let mut order = vec![0];
    let mut next = 0;
    while next < order.len() {
        let p = &sel.packages[order[next]];
        let dev: &[(String, usize)] = if next == 0 { &p.dev_requires } else { &[] };
        for (_, t) in p.requires.iter().chain(dev) {
            if !index.contains_key(t) {
                index.insert(*t, u16::try_from(order.len()).map_err(|e| e.to_string())?);
                order.push(*t);
            }
        }
        next += 1;
    }
    let edges = |list: &[(String, usize)]| -> Requires {
        list.iter().map(|(n, t)| (n.clone(), index[t])).collect()
    };
    let mut deps = Vec::new();
    for &i in &order[1..] {
        let p = &sel.packages[i];
        let (sources, name) = sources_of(&p.dir)?;
        deps.push(DiskPackage {
            name,
            identity: p.identity.clone(),
            sources,
            requires: edges(&p.requires),
            fetched: p.fetched.is_some(),
        });
    }
    Ok(Dependencies {
        requires: edges(&first.requires),
        dev_requires: edges(&first.dev_requires),
        deps,
        problems,
    })
}

/// What `dependencies` finds for a package.
#[derive(Default)]
struct Dependencies {
    requires: Requires,
    dev_requires: Requires,
    deps: Vec<DiskPackage>,
    /// The selection's problems, with the line of the root's `hd.toml`
    /// each is at, if any.
    problems: Vec<(Option<u32>, Diag)>,
}

/// A package name for a root without `hd.toml`: its directory's name.
fn default_name(root: &Path) -> String {
    let raw = hd_run::canonicalize(root)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let name: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        format!("p{name}")
    } else {
        name
    }
}

/// The nearest directory at or above `start` that holds `hd.toml`.
fn nearest_manifest(start: &Path) -> Option<PathBuf> {
    let start = hd_run::canonicalize(start).ok()?;
    start
        .ancestors()
        .find(|d| d.join("hd.toml").is_file())
        .map(Path::to_path_buf)
}

/// The package directory of package mode (`cli.mode.package.nearest`): the
/// nearest `hd.toml` at or above `start`, when it declares a package. A
/// manifest that does not parse counts as one, so its error is reported.
pub fn package_root(start: &Path) -> Option<PathBuf> {
    let dir = nearest_manifest(start)?;
    match manifest_of(&dir) {
        Ok(Some(m)) if !m.declares_package => None,
        _ => Some(dir),
    }
}

/// The members of workspace mode (`cli.mode.workspace`): when the nearest
/// `hd.toml` at or above `start` is a workspace manifest, its directory and
/// each member's directory, in `members` order.
pub fn workspace_root(start: &Path) -> Option<(PathBuf, Vec<PathBuf>)> {
    let dir = nearest_manifest(start)?;
    let m = manifest_of(&dir).ok()??;
    if m.declares_package {
        return None;
    }
    let members = m.workspace?.members.iter().map(|p| dir.join(p)).collect();
    Some((dir, members))
}

/// The workspace a command finds from `start`: at its root
/// (`cli.mode.workspace`), or around the package of package mode when the
/// nearest workspace manifest above it lists the package as a member
/// (`cli.mode.member`). Its directory and its members' directories.
fn enclosing_workspace(start: &Path) -> Option<(PathBuf, Vec<PathBuf>)> {
    if let Some(ws) = workspace_root(start) {
        return Some(ws);
    }
    let package = package_root(start)?;
    let (root, members) = workspace_root(package.parent()?)?;
    members
        .iter()
        .any(|m| hd_run::canonicalize(m).ok().as_ref() == Some(&package))
        .then_some((root, members))
}

/// The members that `-p NAME` flags select (`cli.workspace.select.*`): by
/// package name, wherever the command finds the workspace, each with its
/// directory below the workspace root; a NAME that names no member is an
/// error.
pub fn select_members(start: &Path, names: &[String]) -> Result<Vec<(PathBuf, String)>, String> {
    let Some((root, members)) = enclosing_workspace(start) else {
        return Err(
            "`-p NAME` selects a workspace member, and no workspace is at or above here".to_owned(),
        );
    };
    let by_name: Vec<(PathBuf, String, String)> = members
        .into_iter()
        .map(|dir| {
            let name = package_name(&dir).unwrap_or_default();
            let rel = dir
                .strip_prefix(&root)
                .unwrap_or(&dir)
                .to_string_lossy()
                .replace('\\', "/");
            (dir, rel, name)
        })
        .collect();
    let mut out = Vec::new();
    for n in names {
        match by_name.iter().find(|(_, _, name)| name == n) {
            Some((dir, rel, _)) => {
                if !out.iter().any(|(d, _): &(PathBuf, String)| d == dir) {
                    out.push((dir.clone(), rel.clone()));
                }
            }
            None => {
                return Err(format!(
                    "`-p {n}` names no member of the workspace at {}; its members are {}",
                    root.display(),
                    by_name
                        .iter()
                        .map(|(_, _, name)| format!("`{name}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    Ok(out)
}

/// Reads `-p NAME` or `--package NAME` (or `--package=NAME`) at `args[i]`:
/// the name and how many words it took, or `None` for another word.
pub fn package_flag(
    args: &[std::ffi::OsString],
    i: usize,
) -> Option<Result<(String, usize), String>> {
    let text = args.get(i)?.to_string_lossy();
    if text == "-p" || text == "--package" {
        return Some(
            args.get(i + 1)
                .map(|v| (v.to_string_lossy().into_owned(), 2))
                .ok_or_else(|| format!("`{text}` needs a NAME")),
        );
    }
    text.strip_prefix("--package=")
        .map(|v| Ok((v.to_owned(), 1)))
}

/// A directory of a workspace's `members` or `exclude` list, normalized for
/// comparison: no `./` and no trailing `/`.
fn listed(list: &[String], rel: &str) -> bool {
    list.iter().any(|p| {
        let p = p.trim_end_matches('/');
        p.strip_prefix("./").unwrap_or(p) == rel
    })
}

/// `cli.mode.member.unlisted`: the package at `root` lies under a
/// workspace manifest that neither lists it in `members` nor in `exclude`.
/// The error names that manifest.
fn unlisted_member(root: &Path) -> Result<Option<Diag>, String> {
    let root = hd_run::canonicalize(root).map_err(|e| e.to_string())?;
    for dir in root.ancestors().skip(1) {
        let Some(m) = manifest_of(dir)? else { continue };
        let Some(w) = m.workspace.filter(|_| !m.declares_package) else {
            continue;
        };
        let rel = root
            .strip_prefix(dir)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if listed(&w.members, &rel) || listed(&w.exclude, &rel) {
            return Ok(None);
        }
        let file = format!("{}hd.toml", "../".repeat(rel.split('/').count()));
        let text = std::fs::read_to_string(dir.join("hd.toml")).map_err(|e| e.to_string())?;
        let mut d = Diag::error(
            Some(Code::InvalidManifest),
            &format!(
                "this package lies under the workspace manifest `{}`, which neither lists `{rel}` in `members` nor in `exclude`; add it to one",
                dir.join("hd.toml").display()
            ),
        )
        .at(&file, Some(w.line as usize));
        for (key, message) in [
            ("members", format!("add `{rel}` to `members`")),
            ("exclude", format!("add `{rel}` to `exclude`")),
        ] {
            if let Some(new) = hd_project::edit::add_to_workspace_list(&text, key, &rel)
                && let Some(edit) = text_edit(&file, &text, &new)
            {
                d.fixes.push(Fix {
                    message,
                    edits: vec![edit],
                    exact: false,
                });
            }
        }
        return Ok(Some(d));
    }
    Ok(None)
}

/// The one edit that turns `old` into `new`: it replaces what lies between
/// their shared start and shared end.
fn text_edit(file: &str, old: &str, new: &str) -> Option<Edit> {
    let mut start = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !(old.is_char_boundary(start) && new.is_char_boundary(start)) {
        start -= 1;
    }
    let room = old.len().min(new.len()) - start;
    let mut tail = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    while !(old.is_char_boundary(old.len() - tail) && new.is_char_boundary(new.len() - tail)) {
        tail -= 1;
    }
    Some(Edit {
        file: file.to_owned(),
        start: u32::try_from(start).ok()?,
        end: u32::try_from(old.len() - tail).ok()?,
        text: new[start..new.len() - tail].to_owned(),
    })
}

/// The package name of a root directory: from its `hd.toml`.
pub fn package_name(root: &Path) -> Result<String, String> {
    match std::fs::read_to_string(root.join("hd.toml")) {
        Ok(text) => Ok(parse_manifest(&text)
            .map_err(|e| format!("hd.toml: {e}"))?
            .name),
        Err(_) => Ok(default_name(root)),
    }
}

/// The sources and the package name of a root directory. A package with a
/// source root has the files under it, its test root and `tasks`
/// (`module.manifest.source-root`, `cli.task.file`), so a path dependency
/// in a directory of its own is no part of it. A directory without that
/// layout has every file under it.
pub fn sources_of(root: &Path) -> Result<(DiskSources, String), String> {
    let package = package_name(root)?;
    let src = source_dir(root);
    let mut files = Vec::new();
    if root.join("hd.toml").is_file() && root.join(&src).is_dir() {
        for dir in [src.as_str(), "tasks", "tests"] {
            if root.join(dir).is_dir() {
                walk(root, &root.join(dir), &mut files)?;
            }
        }
    } else {
        walk(root, root, &mut files)?;
    }
    let mut sources = DiskSources {
        root: root.to_path_buf(),
        files: Vec::new(),
        src: (src != "src").then_some(src),
        synthetic: Vec::new(),
    };
    sources.files = files
        .into_iter()
        .map(|e| SourceEntry {
            path: sources.listed(&e.path),
            size: e.size,
        })
        .collect();
    Ok((sources, package))
}

/// Whether FILE, canonical, lies under one of the roots of the package at
/// `root`: the source root, the test root, or `tasks`
/// (`cli.package.no-root`). A package with no source directory takes every
/// file under it, as `sources_of` does.
pub fn under_root(root: &Path, full: &Path) -> bool {
    let src = source_dir(root);
    if !root.join(&src).is_dir() {
        return true;
    }
    [src.as_str(), "tasks", "tests"]
        .iter()
        .any(|dir| full.starts_with(root.join(dir)))
}

/// Whether FILE is a single-file program for `hd check` and `hd test`
/// (`cli.file.check-test`, `cli.package.no-root`): no package holds it, or
/// the package holds it under no root. `full` is canonical.
pub fn is_alone(full: &Path) -> bool {
    let dir = full.parent().unwrap_or(Path::new("."));
    package_root(dir).is_none_or(|root| !under_root(&root, full))
}

/// FILE as a single-file program, whether or not a package holds it
/// (`cli.file.run`): only FILE is a source, and it may use only std
/// (`module.single-file.definition`).
pub fn load_file(target: &Path) -> Result<Program, String> {
    if target.extension().is_none_or(|x| x != "hd") {
        return Err(format!("{}: not an .hd file", target.display()));
    }
    let root = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = target
        .file_name()
        .ok_or("not a file")?
        .to_string_lossy()
        .into_owned();
    let package = package_name(root)?;
    let as_written = Some(target.to_string_lossy().into_owned());
    let size = std::fs::metadata(target).map_or(0, |m| m.len());
    let files = vec![SourceEntry {
        path: name.clone(),
        size,
    }];
    Ok(Program {
        sources: DiskSources {
            root: root.to_path_buf(),
            files,
            src: None,
            synthetic: Vec::new(),
        },
        package,
        entry: module_below(&name),
        requires: Vec::new(),
        dev_requires: Vec::new(),
        deps: Vec::new(),
        problems: Vec::new(),
        single_file: true,
        as_written,
        capabilities: match package_root(root) {
            Some(r) => capabilities(&r)?,
            None => Vec::new(),
        },
        package_dir: package_root(root),
        entries: Vec::new(),
    })
}

/// A single-file program read from standard input (`cli.stdin.program`):
/// the text as a synthetic `main.hd`, alone as a file with no `hd.toml`
/// above it is: no package sources, requirements, or capabilities.
pub fn load_stdin(text: &str) -> Result<Program, String> {
    let root = std::env::current_dir().unwrap_or(PathBuf::from("."));
    let package = package_name(&root)?;
    let mut sources = DiskSources {
        root,
        files: Vec::new(),
        src: None,
        synthetic: Vec::new(),
    };
    sources.add_synthetic("main.hd", text);
    Ok(Program {
        sources,
        package,
        entry: module_below("main.hd"),
        requires: Vec::new(),
        dev_requires: Vec::new(),
        deps: Vec::new(),
        problems: Vec::new(),
        single_file: true,
        as_written: None,
        capabilities: Vec::new(),
        package_dir: None,
        entries: Vec::new(),
    })
}

/// The scope of a REPL session started in `cwd`: the package of package
/// mode, whose `src/lib.hd`, modules and dependencies the session uses
/// (`cli.repl.package.*`), or outside any package, and at a workspace
/// root, no sources: the session uses only `std` (`cli.repl.outside`,
/// `cli.workspace.repl`). The session's inputs are added as synthetic
/// `repl#N` files.
pub fn load_session(cwd: &Path) -> Result<Program, String> {
    if let Some(root) = package_root(cwd) {
        return load_package(&root, "src/lib.hd");
    }
    Ok(Program {
        sources: DiskSources {
            root: cwd.to_path_buf(),
            files: Vec::new(),
            src: None,
            synthetic: Vec::new(),
        },
        package: SESSION_PACKAGE.to_owned(),
        entry: String::new(),
        requires: Vec::new(),
        dev_requires: Vec::new(),
        deps: Vec::new(),
        problems: Vec::new(),
        single_file: false,
        as_written: None,
        capabilities: Vec::new(),
        package_dir: None,
        entries: Vec::new(),
    })
}

/// The package name of a REPL session outside any package. Its inputs are
/// files of no folder, so the name never meets a directory's.
const SESSION_PACKAGE: &str = "repl";

/// The whole package at `root`, with the packages it requires, and the
/// package-relative file `entry` as its entry module.
pub fn load_package(root: &Path, entry: &str) -> Result<Program, String> {
    if let Some(d) = manifest_syntax_problem(root) {
        // Nothing else of the package can be read: the program has no
        // sources, and its one problem stops the command.
        return Ok(Program {
            sources: DiskSources {
                root: root.to_path_buf(),
                files: Vec::new(),
                src: None,
                synthetic: Vec::new(),
            },
            package: default_name(root),
            entry: module_below(entry),
            requires: Vec::new(),
            dev_requires: Vec::new(),
            deps: Vec::new(),
            problems: vec![d],
            single_file: false,
            as_written: None,
            capabilities: Vec::new(),
            package_dir: Some(root.to_path_buf()),
            entries: Vec::new(),
        });
    }
    let (sources, package) = sources_of(root)?;
    let Dependencies {
        requires,
        dev_requires,
        deps,
        problems,
    } = dependencies(root)?;
    let problems = manifest_problems(root, &package, problems)?;
    let entries = executables(root, &package)
        .iter()
        .filter(|e| e.file.starts_with("src/"))
        .map(|e| module_below(&e.file))
        .collect();
    Ok(Program {
        sources,
        package,
        entry: module_below(entry),
        requires,
        dev_requires,
        deps,
        problems,
        single_file: false,
        as_written: None,
        capabilities: capabilities(root)?,
        package_dir: Some(root.to_path_buf()),
        entries,
    })
}

/// A TOML syntax error in the package's `hd.toml`, as the diagnostic
/// `invalid-manifest` at the position it starts at
/// (`cli.manifest.invalid.syntax`).
pub fn manifest_syntax_problem(root: &Path) -> Option<Diag> {
    let text = std::fs::read_to_string(root.join("hd.toml")).ok()?;
    let e = parse_manifest(&text).err()?;
    let mut d = Diag::error(
        Some(Code::InvalidManifest),
        &format!("`hd.toml` is not valid TOML: {}", e.message),
    )
    .at("hd.toml", e.at.map(|(l, _)| l as usize));
    if let Some((_, c)) = e.at {
        d.column = Some(c as usize);
    }
    Some(d)
}

/// The valid keys of a package's `[capabilities]` table (`cli.cap.table`);
/// `manifest_problems` reports the others.
fn capabilities(root: &Path) -> Result<Vec<(String, Grant)>, String> {
    Ok(valid_grants(
        manifest_of(root)?
            .map(|m| m.capabilities)
            .unwrap_or_default(),
    ))
}

/// The valid keys of a package's `[test.capabilities]` table, which the
/// test grant reads instead of `[capabilities]`
/// (`cli.test.env.grant.table`).
pub fn test_capabilities(root: &Path) -> Result<Vec<(String, Grant)>, String> {
    Ok(valid_grants(
        manifest_of(root)?
            .map(|m| m.test_capabilities)
            .unwrap_or_default(),
    ))
}

/// The entries of a grant table whose key and value are valid.
fn valid_grants(table: Vec<(String, Option<Grant>, u32)>) -> Vec<(String, Grant)> {
    table
        .into_iter()
        .filter_map(|(k, g, _)| {
            let g = g?;
            hd_project::grant_problem(&k, &g)
                .is_none()
                .then_some((k, g))
        })
        .collect()
}

/// Whether `dir` holds a manifest that declares a package
/// (`module.path-dep.no-package`).
pub(crate) fn declares_package(dir: &Path) -> Result<bool, String> {
    Ok(manifest_of(dir)?.is_some_and(|m| !m.name.is_empty()))
}

/// What the root manifest breaks, in line order: the rules of the manifest
/// alone (`hd_project::problems`), then those that need the disk: a path
/// requirement's package and library (`module.path-dep.no-package`,
/// `cli.dep.no-library`), the selection's `dependency` problems, such as a
/// missing or differing `hd.sum` entry (`cli.dep.missing-sum`,
/// `cli.dep.verify`), and a task named as an executable
/// (`cli.task.name-clash`).
fn manifest_problems(
    root: &Path,
    package: &str,
    dependency: Vec<(Option<u32>, Diag)>,
) -> Result<Vec<Diag>, String> {
    const FILE: &str = "hd.toml";
    let Some(m) = manifest_of(root)? else {
        return Ok(Vec::new());
    };
    let own = hd_project::problems(&m, TOOLCHAIN);
    let mut out: Vec<(Option<u32>, Diag)> = dependency;
    for r in m.dependencies.iter().chain(&m.dev_dependencies) {
        if own
            .iter()
            .any(|p| p.line == Some(r.line) && p.severity == Severity::Error)
        {
            continue;
        }
        let Some(dir) = &r.path else { continue };
        let dir_path = root.join(dir);
        let message = if !declares_package(&dir_path)? {
            format!(
                "`{}` requires the directory `{dir}`, which holds no package: it has no `hd.toml` with a `[package]` section",
                r.key
            )
        } else if !dir_path
            .join(source_dir(&dir_path))
            .join("lib.hd")
            .is_file()
        {
            format!(
                "`{}` requires the package in `{dir}`, which has no library (`src/lib.hd`), so no package can depend on it",
                r.key
            )
        } else {
            continue;
        };
        out.push((
            Some(r.line),
            Diag::error(Some(Code::InvalidRequirement), &message).at(FILE, Some(r.line as usize)),
        ));
    }
    for p in own {
        out.push((
            p.line,
            Diag::new(p.code, p.severity, &p.message).at(FILE, p.line.map(|l| l as usize)),
        ));
    }
    out.extend(executable_problems(root, &m));
    out.sort_by_key(|(line, _)| *line);
    if let Some(d) = unlisted_member(root)? {
        out.insert(0, (None, d));
    }
    let executables = executables(root, package);
    for t in tasks(root) {
        if executables.iter().any(|e| e.name == t.name) {
            out.push((
                None,
                Diag::error(
                    Some(Code::DuplicateExecutableName),
                    &format!(
                        "the task `{}` and an executable have the same name; rename `{}`",
                        t.name, t.file
                    ),
                )
                .at(FILE, None),
            ));
        }
    }
    Ok(out.into_iter().map(|(_, d)| d).collect())
}

/// The `[[executable]]` rules that need the disk or the whole table:
/// a `module` that names no module (`cli.exe.missing-module`), two
/// executables with one name (`cli.exe.several`), and a `src/main.hd` that
/// no table names (`cli.exe.main-unlisted`), whose fix-it adds a table for
/// it.
fn executable_problems(root: &Path, m: &Manifest) -> Vec<(Option<u32>, Diag)> {
    const FILE: &str = "hd.toml";
    let mut out = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    for e in &m.executables {
        let (Some(name), Some(module)) = (&e.name, &e.module) else {
            continue;
        };
        if module_file(root, module).is_none() {
            out.push((
                Some(e.line),
                Diag::error(
                    Some(Code::MissingEntryPoint),
                    &format!(
                        "the executable `{name}` names the module `{module}`, and the package has no `{}/{}.hd`",
                        source_dir(root),
                        module.replace('.', "/")
                    ),
                )
                .at(FILE, Some(e.line as usize)),
            ));
        }
        if names.contains(&name.as_str()) {
            out.push((
                Some(e.line),
                Diag::error(
                    Some(Code::DuplicateExecutableName),
                    &format!("two executables are named `{name}`; rename one"),
                )
                .at(FILE, Some(e.line as usize)),
            ));
        }
        names.push(name);
    }
    let listed = m
        .executables
        .iter()
        .any(|e| e.module.as_deref() == Some("main"));
    if !m.executables.is_empty() && !listed && root.join(source_dir(root)).join("main.hd").is_file()
    {
        let end = std::fs::metadata(root.join(FILE))
            .map_or(0, |f| u32::try_from(f.len()).unwrap_or(u32::MAX));
        let mut d = Diag::error(
            Some(Code::UnlistedEntry),
                "`src/main.hd` is no executable, since `hd.toml` has `[[executable]]` tables and none names module `main`; add one for it, or rename the file",
        )
        .at(FILE, None);
        d.fixes.push(Fix {
            message: format!(
                "add an `[[executable]]` table for `src/main.hd` named `{}`",
                m.name
            ),
            edits: vec![Edit {
                file: FILE.to_owned(),
                start: end,
                end,
                text: format!(
                    "\n[[executable]]\nname = \"{}\"\nmodule = \"main\"\n",
                    m.name
                ),
            }],
            exact: true,
        });
        out.push((None, d));
    }
    out
}

/// The file of the module at `module` below the source root: `src/a/b.hd`,
/// or `src/a/b/mod.hd`, whichever exists.
fn module_file(root: &Path, module: &str) -> Option<String> {
    let below = module.replace('.', "/");
    let src = source_dir(root);
    [format!("{below}.hd"), format!("{below}/mod.hd")]
        .into_iter()
        .find(|f| root.join(&src).join(f).is_file())
        .map(|f| format!("src/{f}"))
}

/// What `hd run NAME` can run: its name, the package-relative file of its
/// entry module, and that module's path below the source root.
pub struct Runnable {
    pub name: String,
    pub file: String,
    pub is_task: bool,
}

/// The package's executables (`cli.exe.*`): one per `[[executable]]`
/// table whose module exists; with no table, the default one,
/// `src/main.hd` (or `main.hd` at the package directory), named after the
/// package (`cli.exe.default-main`, `cli.exe.default-name`).
pub fn executables(root: &Path, package: &str) -> Vec<Runnable> {
    let tables = manifest_of(root)
        .ok()
        .flatten()
        .map(|m| m.executables)
        .unwrap_or_default();
    if !tables.is_empty() {
        return tables
            .into_iter()
            .filter_map(|e| {
                Some(Runnable {
                    file: module_file(root, e.module.as_deref()?)?,
                    name: e.name?,
                    is_task: false,
                })
            })
            .collect();
    }
    let src = source_dir(root);
    [
        ("src/main.hd", root.join(&src).join("main.hd")),
        ("main.hd", root.join("main.hd")),
    ]
    .into_iter()
    .find(|(_, disk)| disk.is_file())
    .map(|(f, _)| Runnable {
        name: package.to_owned(),
        file: f.to_owned(),
        is_task: false,
    })
    .into_iter()
    .collect()
}

/// The package's tasks: each file `tasks/NAME.hd` (`cli.task.file`).
pub fn tasks(root: &Path) -> Vec<Runnable> {
    let Ok(entries) = std::fs::read_dir(root.join("tasks")) else {
        return Vec::new();
    };
    let mut out: Vec<Runnable> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "hd"))
        .filter_map(|p| {
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            Some(Runnable {
                file: format!("tasks/{stem}.hd"),
                name: stem,
                is_task: true,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
