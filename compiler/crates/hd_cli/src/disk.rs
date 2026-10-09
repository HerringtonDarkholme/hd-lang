//! The disk `SourceSet` and program discovery: the package root, its
//! name from `hd.toml`, and the entry module.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hd_diag::{Code, Severity};
use hd_driver::{Dependency, Packages};
use hd_project::{Grant, Manifest, Role, SourceEntry, SourceSet, module_below, parse_manifest};

use crate::report::{Diag, Edit, Fix};

/// This toolchain's version, which a manifest's `[package] hd` minimum is
/// compared with (`module.toolchain.graph-minimum`).
const TOOLCHAIN: &str = env!("CARGO_PKG_VERSION");

/// Every `.hd` file under a root, read on demand.
pub struct DiskSources {
    root: PathBuf,
    files: Vec<SourceEntry>,
}

impl DiskSources {
    /// Keeps the files whose package-relative path `keep` accepts.
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.files.retain(|e| keep(&e.path));
    }
}

impl SourceSet for DiskSources {
    fn list(&self) -> Vec<SourceEntry> {
        self.files.clone()
    }
    fn read(&self, path: &str) -> Option<Arc<[u8]>> {
        std::fs::read(self.root.join(path)).ok().map(Arc::from)
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<SourceEntry>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.starts_with('.') || name == "build" {
            continue;
        }
        if p.is_dir() {
            walk(root, &p, out)?;
        } else if p.extension().is_some_and(|x| x == "hd") {
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
    /// What its manifest breaks (commands.md §7.1 step 1); any error stops
    /// the command before compiling.
    pub problems: Vec<Diag>,
    /// A single-file program's FILE as the command line wrote it
    /// (`cli.json.diagnostic.file`).
    pub as_written: Option<String>,
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
    pub sources: DiskSources,
    pub requires: Requires,
}

impl Program {
    /// Narrows the sources to one program's: the library, the executables
    /// and `entry_file` (`cli.check.default`, `module.test.code`). Test
    /// code and the other tasks are no part of a program that `hd run` or
    /// `hd build` builds.
    pub fn only_program(&mut self, entry_file: &str) {
        self.sources.retain(|path| {
            path == entry_file || matches!(hd_project::role_of(path), Role::Lib | Role::Exe)
        });
    }

    /// The packages around the root, as the driver takes them.
    pub fn packages(&self) -> Packages<'_> {
        Packages {
            requires: self.requires.clone(),
            deps: self
                .deps
                .iter()
                .map(|d| Dependency {
                    name: d.name.clone(),
                    sources: &d.sources,
                    requires: d.requires.clone(),
                })
                .collect(),
            entries: self.entries.clone(),
            dev_requires: self.dev_requires.clone(),
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

/// The packages a root's path requirements reach, transitively
/// (`module.path-dep.form`), in the order they are first reached, and the
/// root's own requirements and dev requirements. Only the root's
/// `[dev-dependencies]` are read (`module.select.dev-dependencies`). One
/// directory is one package, so a requirement that reaches a package read
/// before names it again: a cycle stays finite, and the build reports it
/// (`module.cycle.package`). A host requirement needs a fetch, and a path
/// with no manifest is no package; neither is read here.
fn dependencies(root: &Path) -> Result<(Requires, Requires, Vec<DiskPackage>), String> {
    let Some(manifest) = manifest_of(root)? else {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    };
    let mut dirs: Vec<PathBuf> = vec![std::fs::canonicalize(root).map_err(|e| e.to_string())?];
    let mut requires: Vec<Requires> = Vec::new();
    let mut dev = Vec::new();
    let mut manifests = vec![manifest];
    let mut deps: Vec<DiskPackage> = Vec::new();
    let mut next = 0;
    while next < dirs.len() {
        let mut own = Vec::new();
        let reqs = manifests[next].dependencies.clone();
        let dev_reqs = if next == 0 {
            manifests[0].dev_dependencies.clone()
        } else {
            Vec::new()
        };
        for (is_dev, r) in reqs
            .iter()
            .map(|r| (false, r))
            .chain(dev_reqs.iter().map(|r| (true, r)))
        {
            let Some(path) = &r.path else { continue };
            let Ok(dir) = std::fs::canonicalize(dirs[next].join(path)) else {
                continue;
            };
            let index = if let Some(i) = dirs.iter().position(|d| *d == dir) {
                i
            } else {
                let Some(m) = manifest_of(&dir)? else {
                    continue;
                };
                let (sources, name) = sources_of(&dir)?;
                deps.push(DiskPackage {
                    name,
                    sources,
                    requires: Vec::new(),
                });
                dirs.push(dir);
                manifests.push(m);
                dirs.len() - 1
            };
            let edge = (r.name(), u16::try_from(index).map_err(|e| e.to_string())?);
            if is_dev {
                dev.push(edge);
            } else {
                own.push(edge);
            }
        }
        requires.push(own);
        next += 1;
    }
    let mut requires = requires.into_iter();
    let root_requires = requires.next().unwrap_or_default();
    for (d, r) in deps.iter_mut().zip(requires) {
        d.requires = r;
    }
    Ok((root_requires, dev, deps))
}

/// A package name for a root without `hd.toml`: its directory's name.
fn default_name(root: &Path) -> String {
    let raw = std::fs::canonicalize(root)
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
    let start = std::fs::canonicalize(start).ok()?;
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
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
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
        return Ok(Some(Diag::error(
            None,
            &format!(
                "this package lies under the workspace manifest `{}`, which neither lists `{rel}` in `members` nor in `exclude`; add it to one",
                dir.join("hd.toml").display()
            ),
        )));
    }
    Ok(None)
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
    let mut files = Vec::new();
    if root.join("hd.toml").is_file() && root.join("src").is_dir() {
        for dir in ["src", "tasks", "tests"] {
            if root.join(dir).is_dir() {
                walk(root, &root.join(dir), &mut files)?;
            }
        }
    } else {
        walk(root, root, &mut files)?;
    }
    Ok((
        DiskSources {
            root: root.to_path_buf(),
            files,
        },
        package,
    ))
}

/// A single-file program, or a module of the package that holds FILE:
/// without an `hd.toml` above FILE, only FILE is a source (`cli.file.run`).
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
    let mut files = Vec::new();
    let alone = package_root(root).is_none();
    let as_written = alone.then(|| target.to_string_lossy().into_owned());
    if alone {
        let size = std::fs::metadata(target).map_or(0, |m| m.len());
        files.push(SourceEntry {
            path: name.clone(),
            size,
        });
    } else {
        walk(root, root, &mut files)?;
    }
    Ok(Program {
        sources: DiskSources {
            root: root.to_path_buf(),
            files,
        },
        package,
        entry: module_below(&name),
        requires: Vec::new(),
        dev_requires: Vec::new(),
        deps: Vec::new(),
        problems: Vec::new(),
        as_written,
        capabilities: match package_root(root) {
            Some(r) => capabilities(&r)?,
            None => Vec::new(),
        },
        package_dir: package_root(root),
        entries: Vec::new(),
    })
}

/// The whole package at `root`, with the packages it requires, and the
/// package-relative file `entry` as its entry module.
pub fn load_package(root: &Path, entry: &str) -> Result<Program, String> {
    let (sources, package) = sources_of(root)?;
    let problems = manifest_problems(root, &package)?;
    let (requires, dev_requires, deps) = dependencies(root)?;
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
        as_written: None,
        capabilities: capabilities(root)?,
        package_dir: Some(root.to_path_buf()),
        entries,
    })
}

/// The valid keys of a package's `[capabilities]` table (`cli.cap.table`);
/// `manifest_problems` reports the others.
fn capabilities(root: &Path) -> Result<Vec<(String, Grant)>, String> {
    Ok(manifest_of(root)?
        .map(|m| m.capabilities)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, g, _)| {
            let g = g?;
            hd_project::grant_problem(&k, &g)
                .is_none()
                .then_some((k, g))
        })
        .collect())
}

/// Whether `dir` holds a manifest that declares a package
/// (`module.path-dep.no-package`).
fn declares_package(dir: &Path) -> Result<bool, String> {
    Ok(manifest_of(dir)?.is_some_and(|m| !m.name.is_empty()))
}

/// What the root manifest breaks, in line order: the rules of the manifest
/// alone (`hd_project::problems`), then those that need the disk: a path
/// requirement's package and library (`module.path-dep.no-package`,
/// `cli.dep.no-library`), a dependency requirement's `hd.sum` entry
/// (`cli.dep.missing-sum`), and a task named as an executable
/// (`cli.task.name-clash`).
fn manifest_problems(root: &Path, package: &str) -> Result<Vec<Diag>, String> {
    const FILE: &str = "hd.toml";
    let Some(m) = manifest_of(root)? else {
        return Ok(Vec::new());
    };
    let own = hd_project::problems(&m, TOOLCHAIN);
    let mut out: Vec<(Option<u32>, Diag)> = Vec::new();
    let sum = std::fs::read_to_string(root.join("hd.sum")).unwrap_or_default();
    for r in m.dependencies.iter().chain(&m.dev_dependencies) {
        if own
            .iter()
            .any(|p| p.line == Some(r.line) && p.severity == Severity::Error)
        {
            continue;
        }
        let why = if let Some(dir) = &r.path {
            let dir_path = root.join(dir);
            if !declares_package(&dir_path)? {
                Some((
                    Code::InvalidRequirement,
                    format!(
                        "`{}` requires the directory `{dir}`, which holds no package: it has no `hd.toml` with a `[package]` section",
                        r.key
                    ),
                ))
            } else if !dir_path.join("src/lib.hd").is_file() {
                Some((
                    Code::InvalidRequirement,
                    format!(
                        "`{}` requires the package in `{dir}`, which has no library (`src/lib.hd`), so no package can depend on it",
                        r.key
                    ),
                ))
            } else {
                None
            }
        } else {
            let entry = format!("{} ", r.text);
            (!sum.lines().any(|l| l.starts_with(&entry))).then(|| {
                (
                    Code::MissingSumEntry,
                    format!(
                        "`{}` has no `hd.sum` entry; run `hd fetch` to fetch it and record its hash, or `hd add` to require it",
                        r.text
                    ),
                )
            })
        };
        if let Some((code, message)) = why {
            out.push((
                Some(r.line),
                Diag::error(Some(code), &message).at(FILE, Some(r.line as usize)),
            ));
        }
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
                        "the executable `{name}` names the module `{module}`, and the package has no `src/{}.hd`",
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
    if !m.executables.is_empty() && !listed && root.join("src/main.hd").is_file() {
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
        });
        out.push((None, d));
    }
    out
}

/// The file of the module at `module` below the source root: `src/a/b.hd`,
/// or `src/a/b/mod.hd`, whichever exists.
fn module_file(root: &Path, module: &str) -> Option<String> {
    let below = module.replace('.', "/");
    [format!("src/{below}.hd"), format!("src/{below}/mod.hd")]
        .into_iter()
        .find(|f| root.join(f).is_file())
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
    ["src/main.hd", "main.hd"]
        .iter()
        .find(|f| root.join(f).is_file())
        .map(|f| Runnable {
            name: package.to_owned(),
            file: (*f).to_owned(),
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
