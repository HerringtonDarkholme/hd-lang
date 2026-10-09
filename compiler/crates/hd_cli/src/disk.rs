//! The disk `SourceSet` and program discovery: the package root, its
//! name from `hd.toml`, and the entry module.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hd_diag::Code;
use hd_driver::{Dependency, Packages};
use hd_project::{Manifest, SourceEntry, SourceSet, module_below, parse_manifest};

use crate::report::Diag;

/// This toolchain's version, which a manifest's `[package] hd` minimum is
/// compared with (`module.toolchain.graph-minimum`).
const TOOLCHAIN: &str = env!("CARGO_PKG_VERSION");

/// Every `.hd` file under a root, read on demand.
pub struct DiskSources {
    root: PathBuf,
    files: Vec<SourceEntry>,
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
    /// The packages its path requirements reach.
    pub deps: Vec<DiskPackage>,
    /// What its manifest breaks (commands.md §7.1 step 1); any error stops
    /// the command before compiling.
    pub problems: Vec<Diag>,
    /// A single-file program's FILE as the command line wrote it
    /// (`cli.json.diagnostic.file`).
    pub as_written: Option<String>,
}

/// A package that a path requirement reaches.
pub struct DiskPackage {
    pub name: String,
    pub sources: DiskSources,
    pub requires: Requires,
}

impl Program {
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
/// (`module.path-dep.form`), in the order they are first reached. One
/// directory is one package, so a requirement that reaches a package read
/// before names it again: a cycle stays finite, and the build reports it
/// (`module.cycle.package`). A host requirement needs a fetch, and a path
/// with no manifest is no package; neither is read here.
fn dependencies(root: &Path) -> Result<(Requires, Vec<DiskPackage>), String> {
    let Some(manifest) = manifest_of(root)? else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut dirs: Vec<PathBuf> = vec![std::fs::canonicalize(root).map_err(|e| e.to_string())?];
    let mut requires: Vec<Requires> = Vec::new();
    let mut manifests = vec![manifest];
    let mut deps: Vec<DiskPackage> = Vec::new();
    let mut next = 0;
    while next < dirs.len() {
        let mut own = Vec::new();
        let reqs = manifests[next].dependencies.clone();
        for r in &reqs {
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
            own.push((r.name(), u16::try_from(index).map_err(|e| e.to_string())?));
        }
        requires.push(own);
        next += 1;
    }
    let mut requires = requires.into_iter();
    let root_requires = requires.next().unwrap_or_default();
    for (d, r) in deps.iter_mut().zip(requires) {
        d.requires = r;
    }
    Ok((root_requires, deps))
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
pub fn package_root(start: &Path) -> Option<PathBuf> {
    let start = std::fs::canonicalize(start).ok()?;
    start
        .ancestors()
        .find(|d| d.join("hd.toml").is_file())
        .map(Path::to_path_buf)
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
        deps: Vec::new(),
        problems: Vec::new(),
        as_written,
    })
}

/// The whole package at `root`, with the packages it requires, and the
/// package-relative file `entry` as its entry module.
pub fn load_package(root: &Path, entry: &str) -> Result<Program, String> {
    let (sources, package) = sources_of(root)?;
    let problems = manifest_problems(root, &package)?;
    let (requires, deps) = dependencies(root)?;
    Ok(Program {
        sources,
        package,
        entry: module_below(entry),
        requires,
        deps,
        problems,
        as_written: None,
    })
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
        if own.iter().any(|p| p.line == Some(r.line)) {
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
            Diag::error(Some(p.code), &p.message).at(FILE, p.line.map(|l| l as usize)),
        ));
    }
    out.sort_by_key(|(line, _)| *line);
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

/// What `hd run NAME` can run: its name and the package-relative file of
/// its entry module.
pub struct Runnable {
    pub name: String,
    pub file: String,
    pub is_task: bool,
}

/// The package's executables: the default one, `src/main.hd` (or `main.hd`
/// at the package directory), named after the package (`cli.exe.default-main`,
/// `cli.exe.default-name`). `[[executable]]` tables are not read yet.
pub fn executables(root: &Path, package: &str) -> Vec<Runnable> {
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
