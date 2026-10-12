//! The version cache (`cli.cache.*`): one directory per user, shared by
//! every package (`cli.cache.shared`). A fetched version lives in
//! `pkg/HOST_PATH@VERSION` (`cli.cache.entry`), read-only once written
//! (`cli.cache.read-only`), and appears only once complete
//! (`cli.cache.complete`). The tree hash computed when the entry was
//! written is kept beside it, in `hash/HOST_PATH@VERSION`
//! (`cli.cache.hash`), so a command compares `hd.sum` with that record
//! instead of hashing the tree again.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::sum::tree_hash;

/// A version's entry: its tree's directory and the tree hash recorded
/// with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub dir: PathBuf,
    pub hash: String,
}

/// The cache directory (`cli.cache.directory`), as the command resolved
/// it.
#[derive(Clone, Debug)]
pub struct Cache {
    root: PathBuf,
}

/// A scratch directory under the cache's `tmp`, on the file system of its
/// entries so that an entry moves into place in one rename. It is removed
/// when dropped.
pub struct Scratch {
    pub path: PathBuf,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = remove_tree(&self.path);
    }
}

impl Cache {
    #[must_use]
    pub fn new(root: PathBuf) -> Cache {
        Cache { root }
    }

    /// `ROOT/SECTION/HOST_PATH@VERSION`; `key` is a checked host path and
    /// version, so each of its segments is a plain name.
    fn place(&self, section: &str, key: &str) -> PathBuf {
        let mut p = self.root.join(section);
        p.extend(key.split('/'));
        p
    }

    /// The directory of the entry `HOST_PATH@VERSION` (`cli.cache.entry`).
    #[must_use]
    pub fn entry_dir(&self, key: &str) -> PathBuf {
        self.place("pkg", key)
    }

    /// The cached entry of `HOST_PATH@VERSION`, or `None` when the cache
    /// lacks it. A tree in place whose hash record is missing, as after a
    /// crash between the two steps of `store`, gets its record now: the
    /// tree is complete once it is in place.
    pub fn cached(&self, key: &str) -> Result<Option<Entry>, String> {
        let dir = self.entry_dir(key);
        if !dir.is_dir() {
            return Ok(None);
        }
        let record = self.place("hash", key);
        if let Ok(text) = std::fs::read_to_string(&record)
            && let Some(hash) = text.strip_suffix('\n').filter(|h| h.starts_with("h1:"))
        {
            return Ok(Some(Entry {
                dir,
                hash: hash.to_owned(),
            }));
        }
        let hash = tree_hash(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        self.write_record(key, &hash)?;
        Ok(Some(Entry { dir, hash }))
    }

    /// A fresh scratch directory.
    pub fn scratch(&self) -> Result<Scratch, String> {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!(
            "{}-{nanos}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        );
        let path = self.root.join("tmp").join(name);
        std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Scratch { path })
    }

    /// Stores the tree in `tree`, a directory of a scratch directory, as
    /// the entry of `HOST_PATH@VERSION`: made read-only, then moved into
    /// place in one rename, then its hash recorded.
    ///
    /// Several `hd` processes may store one version at once. The first
    /// rename wins; a loser leaves its tree to be removed with its scratch
    /// directory and uses the winner's entry, so a record always describes
    /// the tree in place.
    pub fn store(&self, key: &str, tree: &Path) -> Result<Entry, String> {
        let shown = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
        let hash = tree_hash(tree).map_err(|e| shown(tree, e))?;
        make_contents_read_only(tree).map_err(|e| shown(tree, e))?;
        let dir = self.entry_dir(key);
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| shown(parent, e))?;
        }
        match std::fs::rename(tree, &dir) {
            Ok(()) => {
                set_read_only(&dir, true).map_err(|e| shown(&dir, e))?;
                self.write_record(key, &hash)?;
                Ok(Entry { dir, hash })
            }
            Err(e) => match self.cached(key)? {
                Some(entry) => Ok(entry),
                None => Err(shown(&dir, e)),
            },
        }
    }

    /// Writes the hash record of `key`, replacing any in one rename.
    fn write_record(&self, key: &str, hash: &str) -> Result<(), String> {
        let record = self.place("hash", key);
        let parent = record.parent().unwrap_or(&self.root);
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        let scratch = self.scratch()?;
        let staged = scratch.path.join("hash");
        std::fs::write(&staged, format!("{hash}\n"))
            .and_then(|()| std::fs::rename(&staged, &record))
            .map_err(|e| format!("{}: {e}", record.display()))
    }
}

/// Makes every file and directory inside `dir` read-only. `dir` itself
/// stays writable until it is in place, since some systems cannot rename a
/// directory that is not writable.
fn make_contents_read_only(dir: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            make_contents_read_only(&entry.path())?;
            set_read_only(&entry.path(), true)?;
        } else if kind.is_file() {
            set_read_only(&entry.path(), true)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_read_only(path: &Path, read_only: bool) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::fs::symlink_metadata(path)?.is_dir();
    let mode = match (dir, read_only) {
        (true, true) => 0o555,
        (true, false) => 0o755,
        (false, true) => 0o444,
        (false, false) => 0o644,
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_read_only(path: &Path, read_only: bool) -> std::io::Result<()> {
    let mut p = std::fs::metadata(path)?.permissions();
    p.set_readonly(read_only);
    std::fs::set_permissions(path, p)
}

/// Removes a directory that may hold read-only entries, never following a
/// symbolic link.
pub fn remove_tree(path: &Path) -> std::io::Result<()> {
    fn writable(dir: &Path) {
        let _ = set_read_only(dir, false);
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    writable(&e.path());
                } else if cfg!(not(unix)) && e.file_type().is_ok_and(|t| t.is_file()) {
                    let _ = set_read_only(&e.path(), false);
                }
            }
        }
    }
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(m) if m.is_dir() => {
            writable(path);
            std::fs::remove_dir_all(path)
        }
        _ => std::fs::remove_file(path),
    }
}

#[cfg(test)]
mod tests {
    use super::Cache;
    use crate::sum::tree_hash;

    /// A directory under the workspace's `target/tmp`, removed on drop.
    fn temp(name: &str) -> hd_testkit::Scratch {
        hd_testkit::Scratch::unmade(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/tmp"),
            &format!("hd-deps-cache-{name}"),
        )
    }

    #[test]
    fn an_entry_is_read_only_and_records_its_hash() {
        let root = temp("store");
        let cache = Cache::new(root.to_path_buf());
        let key = "github.com/acme/text@1.0.0";
        assert_eq!(cache.cached(key).expect("read"), None);
        let scratch = cache.scratch().expect("scratch");
        let tree = scratch.path.join("tree");
        std::fs::create_dir_all(tree.join("src")).expect("mkdir");
        std::fs::write(tree.join("hd.toml"), "[package]\nname = \"text\"\n").expect("write");
        std::fs::write(tree.join("src/lib.hd"), "pub fn f() -> i32: 1\n").expect("write");
        let hash = tree_hash(&tree).expect("hash");
        let entry = cache.store(key, &tree).expect("store");
        drop(scratch);
        assert_eq!(entry.hash, hash);
        assert_eq!(entry.dir, root.join("pkg/github.com/acme/text@1.0.0"));
        assert!(
            std::fs::metadata(entry.dir.join("src/lib.hd"))
                .expect("meta")
                .permissions()
                .readonly()
        );
        assert_eq!(cache.cached(key).expect("read"), Some(entry.clone()));
        // A lost record is written again from the tree in place.
        std::fs::remove_file(root.join("hash/github.com/acme/text@1.0.0")).expect("rm");
        assert_eq!(cache.cached(key).expect("read"), Some(entry));
        // A second store of the same version keeps the first tree.
        let scratch = cache.scratch().expect("scratch");
        let other = scratch.path.join("tree");
        std::fs::create_dir_all(&other).expect("mkdir");
        std::fs::write(other.join("hd.toml"), "other").expect("write");
        assert_eq!(cache.store(key, &other).expect("store").hash, hash);
        drop(scratch);
        assert_eq!(std::fs::read_dir(root.join("tmp")).expect("tmp").count(), 0);
        super::remove_tree(&root).expect("clean");
        assert!(!root.exists());
    }
}
