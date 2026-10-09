//! `hd clean [--cache]` (Cleaning, `cli.clean.*`): the build directory of
//! the package or of each workspace member, or the cache directory's
//! fetched versions and compiled entries. Never source or a manifest.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::{cache_root, disk, fail};

/// The entries of a cache directory that `hd` owns (`cli.clean.cache.entries`).
const ENTRIES: &[&str] = &["pkg", "hash", "obj", "tmp"];

pub(crate) fn command(args: &[OsString]) -> ExitCode {
    let cache = match args {
        [] => false,
        [one] if one == "--cache" => true,
        [one, ..] if one.to_string_lossy().starts_with('-') && one != "--cache" => {
            return fail(&format!(
                "`hd clean` has no option `{}`",
                one.to_string_lossy()
            ));
        }
        // `cli.clean.no-question`: no operand.
        _ => return fail("`hd clean` takes no operand; `hd clean --cache` empties the cache"),
    };
    let result = if cache { clean_cache() } else { clean_build() };
    match result {
        Ok(lines) => {
            for l in lines {
                println!("{l}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

/// `cli.clean.build`, `.build.none`, `.build.workspace`, `.build.package-only`.
fn clean_build() -> Result<Vec<String>, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let dirs: Vec<(PathBuf, String)> = if let Some((ws, members)) = disk::workspace_root(&cwd) {
        members
            .into_iter()
            .map(|m| {
                let rel = m
                    .strip_prefix(&ws)
                    .unwrap_or(&m)
                    .to_string_lossy()
                    .replace('\\', "/");
                (m.join("build"), format!("{rel}/build"))
            })
            .collect()
    } else if let Some(root) = disk::package_root(&cwd) {
        vec![(root.join("build"), "build".to_owned())]
    } else {
        return Err(
            "`hd clean` removes a package's build directory, and no package is at or above here; create one with `hd new`, or empty the cache with `hd clean --cache`"
                .to_owned(),
        );
    };
    let mut out = Vec::new();
    for (dir, shown) in dirs {
        if dir.symlink_metadata().is_ok() {
            remove(&dir)?;
            out.push(format!("removed {shown}"));
        }
    }
    if out.is_empty() {
        out.push("nothing to clean".to_owned());
    }
    Ok(out)
}

/// `cli.clean.cache.*`: the fetched versions under `pkg`, and `hash`,
/// `obj` and `tmp`, after the layout check of `cli.clean.cache.foreign`.
fn clean_cache() -> Result<Vec<String>, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let dir = cwd.join(cache_root());
    let shown = dir.display().to_string();
    let Ok(meta) = std::fs::metadata(&dir) else {
        return Ok(vec![format!("the cache {shown} is empty")]);
    };
    let real = std::fs::canonicalize(&dir).map_err(|e| format!("{shown}: {e}"))?;
    let home = std::env::var_os("HOME").and_then(|h| std::fs::canonicalize(h).ok());
    let foreign = |why: &str| {
        Err(format!(
            "{shown} is no hd cache: {why}; `hd clean --cache` removes nothing"
        ))
    };
    if !meta.is_dir() {
        return foreign("it is not a directory");
    }
    if real.parent().is_none() {
        return foreign("it is the file system root");
    }
    if home.as_ref() == Some(&real) {
        return foreign("it is the home directory");
    }
    let mut present = Vec::new();
    for entry in std::fs::read_dir(&real).map_err(|e| format!("{shown}: {e}"))? {
        let name = entry.map_err(|e| e.to_string())?.file_name();
        let name = name.to_string_lossy().into_owned();
        if !ENTRIES.contains(&name.as_str()) {
            return foreign(&format!("it holds `{name}`, which is not hd's"));
        }
        present.push(name);
    }
    if present.is_empty() {
        return Ok(vec![format!("the cache {shown} is empty")]);
    }
    let mut versions = Vec::new();
    versions_in(&real.join("pkg"), &real.join("pkg"), &mut versions);
    versions.sort();
    for name in &present {
        remove(&real.join(name))?;
    }
    let mut out: Vec<String> = versions.iter().map(|v| format!("removed {v}")).collect();
    out.push(format!(
        "removed {} version{} from {shown}",
        versions.len(),
        if versions.len() == 1 { "" } else { "s" }
    ));
    Ok(out)
}

/// Each fetched version under `pkg`, as `HOST_PATH@VERSION`
/// (`cli.cache.entry`): a directory whose name holds `@`. Symbolic links
/// are not followed.
fn versions_in(pkg: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if entry.file_name().to_string_lossy().contains('@') {
            if let Ok(rel) = path.strip_prefix(pkg) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        } else {
            versions_in(pkg, &path, out);
        }
    }
}

/// Removes `path`, making each directory under it writable first, since
/// cache entries are read-only (`cli.clean.cache.writable`). A symbolic
/// link is removed, never followed.
fn remove(path: &Path) -> Result<(), String> {
    let meta = path
        .symlink_metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.is_dir() {
        make_writable(path);
        std::fs::remove_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))
    } else {
        std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Gives the owner write access to a directory and the directories under
/// it, so their entries can be removed.
fn make_writable(dir: &Path) {
    let Ok(meta) = dir.symlink_metadata() else {
        return;
    };
    if !meta.is_dir() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o700);
        let _ = std::fs::set_permissions(dir, perms);
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                make_writable(&entry.path());
            }
        }
    }
}
