//! Source sets from the file system. A file is a module and its directory is
//! its folder, so a one-file program needs no setup: `hd run hello.hd`
//! compiles `hello.hd` as module `pkg.hello` in the root folder `pkg`.
//!
//! The root folder is the directory of the file (or the directory given).
//! Every `.hd` file directly in it is a module of the root folder. A `use
//! pkg.a.b.m` line brings in the folder `pkg.a.b`, which is the directory
//! `a/b` under the root, with every `.hd` file directly in it; those files'
//! uses are followed the same way. Folders nothing uses are not read.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use hd_iface::folder_of_module;
use hd_syntax::{HeaderSkeleton, skim};

pub struct SourceFile {
    /// Path relative to the root folder, with `/` separators.
    pub path: String,
    pub text: String,
}

/// A loaded program: its source set and the entry module.
pub struct Program {
    pub sources: Vec<SourceFile>,
    pub entry: String,
}

/// The module path of a file: `pkg.` plus its relative path, `/` as `.`.
#[must_use]
pub fn module_path(file: &str) -> String {
    format!("pkg.{}", file.trim_end_matches(".hd").replace('/', "."))
}

/// The module paths named by a source's `use` lines.
#[must_use]
pub fn use_paths(src: &str, sk: &HeaderSkeleton) -> Vec<String> {
    let mut out = Vec::new();
    for &(lo, hi) in &sk.uses {
        let text = &src[lo as usize..hi as usize];
        if let Some(rest) = text.strip_prefix("use ") {
            out.push(rest.split(".{").next().unwrap_or(rest).trim().to_owned());
        }
    }
    out
}

fn hd_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "hd"))
        .collect();
    files.sort();
    Ok(files)
}

fn read_folder(root: &Path, rel: &str, out: &mut Vec<SourceFile>) -> Result<(), String> {
    let dir = if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    for p in hd_files(&dir)? {
        let name = p
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .into_owned();
        let path = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        out.push(SourceFile { path, text });
    }
    Ok(())
}

/// Loads a program from a file (that file is the entry) or a directory (its
/// entry is `main.hd`, or else the one root-folder file with `fn main`).
pub fn load_program(target: &Path) -> Result<Program, String> {
    let (root, entry_file) = if target.is_dir() {
        (target.to_path_buf(), None)
    } else {
        let root = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = target
            .file_name()
            .ok_or("not a file")?
            .to_string_lossy()
            .into_owned();
        if target.extension().is_none_or(|x| x != "hd") {
            return Err(format!("{}: not an .hd file", target.display()));
        }
        (root.to_path_buf(), Some(name))
    };
    let mut sources = Vec::new();
    read_folder(&root, "", &mut sources)?;
    let mut loaded: BTreeSet<String> = BTreeSet::from(["pkg".to_owned()]);
    let mut next = 0;
    while next < sources.len() {
        let src = &sources[next].text;
        let uses = use_paths(src, &skim(src.as_bytes()));
        next += 1;
        for u in uses {
            let folder = folder_of_module(&u).to_owned();
            let Some(rel) = folder.strip_prefix("pkg.") else {
                continue;
            };
            let rel = rel.replace('.', "/");
            if loaded.insert(folder) && root.join(&rel).is_dir() {
                read_folder(&root, &rel, &mut sources)?;
            }
        }
    }
    let entry_file = match entry_file {
        Some(f) => f,
        None if sources.iter().any(|s| s.path == "main.hd") => "main.hd".to_owned(),
        None => {
            let mains: Vec<&SourceFile> = sources
                .iter()
                .filter(|s| {
                    !s.path.contains('/') && s.text.lines().any(|l| l.starts_with("fn main("))
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
    Ok(Program {
        sources,
        entry: module_path(&entry_file),
    })
}
