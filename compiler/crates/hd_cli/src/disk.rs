//! The disk `SourceSet` and program discovery: the package root, its
//! name from `hd.toml`, and the entry module.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hd_project::{SourceEntry, SourceSet, parse_manifest};

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

pub struct Program {
    pub sources: DiskSources,
    pub package: String,
    /// The entry module below the package, as `main`.
    pub entry: String,
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

/// The sources and the package name of a root directory.
pub fn sources_of(root: &Path) -> Result<(DiskSources, String), String> {
    let package = package_name(root)?;
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    Ok((
        DiskSources {
            root: root.to_path_buf(),
            files,
        },
        package,
    ))
}

/// The entry module path of a package-relative file: `src/main.hd` is
/// `src.main`.
fn module_of(rel: &str) -> String {
    rel.trim_end_matches(".hd").replace('/', ".")
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
    if package_root(root).is_none() {
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
        entry: module_of(&name),
    })
}

/// The whole package at `root`, with the package-relative file `entry` as
/// its entry module.
pub fn load_package(root: &Path, entry: &str) -> Result<Program, String> {
    let (sources, package) = sources_of(root)?;
    Ok(Program {
        sources,
        package,
        entry: module_of(entry),
    })
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
