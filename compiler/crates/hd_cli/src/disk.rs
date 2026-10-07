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

/// Finds the package root, its name and the entry module of a target.
pub fn load(target: &Path) -> Result<Program, String> {
    let (root, entry_file) = if target.is_dir() {
        (target.to_path_buf(), None)
    } else {
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
        (root.to_path_buf(), Some(name))
    };
    let package = match std::fs::read_to_string(root.join("hd.toml")) {
        Ok(text) => {
            parse_manifest(&text)
                .map_err(|e| format!("hd.toml: {e}"))?
                .name
        }
        Err(_) => default_name(&root),
    };
    let mut files = Vec::new();
    walk(&root, &root, &mut files)?;
    let sources = DiskSources {
        root: root.clone(),
        files,
    };
    let entry_file = match entry_file {
        Some(f) => f,
        None if sources.files.iter().any(|s| s.path == "main.hd") => "main.hd".to_owned(),
        None => {
            let mains: Vec<&SourceEntry> = sources
                .files
                .iter()
                .filter(|s| !s.path.contains('/'))
                .filter(|s| {
                    sources.read(&s.path).is_some_and(|b| {
                        String::from_utf8_lossy(&b)
                            .lines()
                            .any(|l| l.starts_with("fn main("))
                    })
                })
                .collect();
            match mains.as_slice() {
                [one] => one.path.clone(),
                [] => return Err(format!("{}: no file defines `fn main`", root.display())),
                _ => {
                    return Err(format!(
                        "{}: several files define `fn main`",
                        root.display()
                    ));
                }
            }
        }
    };
    let entry = entry_file.trim_end_matches(".hd").replace('/', ".");
    Ok(Program {
        sources,
        package,
        entry,
    })
}
