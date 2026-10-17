//! The path sandbox (`cli.cap.scope.path`, `cli.cap.scope.path.resolved`).
//!
//! `Grants::covers_path` decides whether a path is granted, on the path
//! resolved at that moment. The operation must then touch that same file,
//! even when the tree changes between the check and the open. So a trait
//! with scope entries opens each granted directory once, from the file
//! system root without following any symbolic link, and keeps the handle
//! for the run. Every operation then resolves under that handle with
//! `cap-std`, which never leaves it: not by `..`, not by an absolute path,
//! and not by a symbolic link, wherever the link points. A path that would
//! leave is refused as `NotGranted`.
//!
//! The path an operation acts on is the resolved path the grant check
//! compared, relative to the entry that covers it. An operation that does
//! not follow a final link (`stat`, `remove`, `rename`) acts on the path
//! with only its parent resolved, which must also lie in a grant. An
//! operation on an entry itself goes through the entry's parent directory,
//! by the entry's name, and never follows a link there, since a link at
//! the entry's name leads to a path the entry does not name.

use std::ffi::OsStr;
use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use hd_run::{Grants, Limit};

/// What `cap-std` reports for a path that would leave its directory, and
/// what this module reports for a symbolic link the sandbox will not follow.
const ESCAPE: &str = "a path led outside of the filesystem";

/// The handles of a program's granted directories, each opened at its
/// first use and kept for the run.
#[derive(Default)]
pub(crate) struct Sandbox {
    open: Mutex<Vec<(PathBuf, Arc<Dir>)>>,
}

/// Where a file operation acts.
pub(crate) enum Place {
    /// By name, with `std::fs`: the trait has no limit.
    Ambient(PathBuf),
    /// Under a granted directory's handle.
    Under {
        dir: Arc<Dir>,
        rel: PathBuf,
        /// `rel` is a granted entry's own name, and `dir` its parent: a
        /// symbolic link there is never followed.
        entry: bool,
        /// The resolved path, so that removing it can forget a handle.
        target: PathBuf,
    },
    /// A granted directory that could not be opened; every operation
    /// fails with this error.
    Unreachable(io::Error),
}

/// What an operation learns of a file.
pub(crate) struct Stat {
    pub(crate) dir: bool,
    pub(crate) link: bool,
    pub(crate) len: u64,
    /// A FIFO, a device or a socket: opening it can wait on another
    /// process. Never set on Windows, where a named pipe goes by its name
    /// (`Place::is_special`).
    pub(crate) special: bool,
}

impl From<std::fs::Metadata> for Stat {
    fn from(st: std::fs::Metadata) -> Self {
        let link = st.file_type().is_symlink();
        Stat {
            dir: st.is_dir(),
            link,
            len: st.len(),
            special: cfg!(unix) && !st.is_file() && !st.is_dir() && !link,
        }
    }
}

impl From<cap_std::fs::Metadata> for Stat {
    fn from(st: cap_std::fs::Metadata) -> Self {
        let link = st.file_type().is_symlink();
        Stat {
            dir: st.is_dir(),
            link,
            len: st.len(),
            special: cfg!(unix) && !st.is_file() && !st.is_dir() && !link,
        }
    }
}

/// Whether `error` reports a path that would leave the sandbox.
pub(crate) fn is_escape(error: &io::Error) -> bool {
    error.kind() == ErrorKind::PermissionDenied
        && error.raw_os_error().is_none()
        && error.to_string() == ESCAPE
}

fn escape() -> io::Error {
    io::Error::new(ErrorKind::PermissionDenied, ESCAPE)
}

/// The same error once more, for a place that fails every operation.
fn again(error: &io::Error) -> io::Error {
    match error.raw_os_error() {
        Some(code) => io::Error::from_raw_os_error(code),
        None => io::Error::new(error.kind(), error.to_string()),
    }
}

impl Sandbox {
    /// Where an operation under `key` acts on `path`, the guest's path
    /// against the case's directory, or `None` when no entry covers it.
    /// `follow` says whether the operation follows a final symbolic link;
    /// `create` makes a missing granted directory and its parents, as
    /// `create_dir_all` does. A trait with no limit acts by name; a total
    /// deny never gets here, since the grant check refused it.
    pub(crate) fn place(
        &self,
        grants: &Grants,
        key: &str,
        path: &Path,
        follow: bool,
        create: bool,
    ) -> Option<Place> {
        // `..` after a missing name is NotFound on every OS: Windows path
        // APIs resolve `..` as text, so the check comes before any access.
        if !create && let Some(error) = before_dot_dot(path) {
            return Some(Place::Unreachable(error));
        }
        let Some(Limit::Entries(entries)) = grants.limit(key) else {
            return Some(Place::Ambient(path.to_owned()));
        };
        if path.as_os_str().is_empty() {
            // Fails as not found, as `std::fs` and Node do, touching nothing.
            return Some(Place::Ambient(PathBuf::new()));
        }
        // The path as the OS resolves it (`cli.cap.scope.path.access`): a
        // `..` after a symbolic link goes up from where the link leads,
        // except on Windows, which resolves every `..` by its text first.
        let path = if cfg!(windows) {
            hd_run::lexical_path(path)
        } else {
            path.to_owned()
        };
        let target = match (follow, path.parent(), path.file_name()) {
            (false, Some(parent), Some(name)) => hd_run::resolve_path(parent).join(name),
            _ => hd_run::resolve_path(&path),
        };
        let entry = entries
            .iter()
            .map(Path::new)
            .filter(|e| target.starts_with(e))
            .max_by_key(|e| e.components().count())?;
        let rel = target.strip_prefix(entry).ok()?.to_owned();
        let place = |dir: io::Result<Arc<Dir>>, rel: PathBuf, entry: bool| match dir {
            Ok(dir) => Place::Under {
                dir,
                rel,
                entry,
                target: target.clone(),
            },
            Err(error) => Place::Unreachable(error),
        };
        Some(
            match (
                rel.as_os_str().is_empty(),
                entry.parent(),
                entry.file_name(),
            ) {
                (true, Some(parent), Some(name)) => {
                    place(walk(parent, create).map(Arc::new), name.into(), true)
                }
                (true, ..) => place(self.granted(entry, create), ".".into(), false),
                (false, ..) => place(self.granted(entry, create), rel, false),
            },
        )
    }

    /// The handle of the granted directory `entry`, opened at its first
    /// use.
    fn granted(&self, entry: &Path, create: bool) -> io::Result<Arc<Dir>> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, dir)) = open.iter().find(|(path, _)| path == entry) {
            return Ok(Arc::clone(dir));
        }
        let dir = Arc::new(walk(entry, create)?);
        open.push((entry.to_owned(), Arc::clone(&dir)));
        Ok(dir)
    }

    /// Drops the handles of granted directories that a `remove` or a
    /// `rename` of `place` takes away, so that a directory made again at
    /// that path is opened afresh. A handle that is still needed is just
    /// opened again.
    pub(crate) fn forget(&self, place: &Place) {
        if let Place::Under { target, .. } = place {
            self.open
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|(path, _)| !path.starts_with(target));
        }
    }
}

/// The error for `path` when a name before one of its `..` is not an
/// existing directory, as in `data/missing/../a.txt`: `NotFound`, or
/// `NotADirectory` for a file. Unix fails such a path itself, but Windows
/// resolves `..` as text and would skip the missing name, so every engine
/// and OS checks it here (#277, `cli.engine.same-behavior`).
fn before_dot_dot(path: &Path) -> Option<io::Error> {
    let mut prefix = PathBuf::new();
    for c in path.components() {
        if c == Component::ParentDir && !prefix.as_os_str().is_empty() {
            match std::fs::metadata(&prefix) {
                Err(error) => return Some(error),
                Ok(st) if !st.is_dir() => return Some(ErrorKind::NotADirectory.into()),
                Ok(_) => {}
            }
        }
        prefix.push(c);
    }
    None
}

/// Opens the directory at the resolved path `path` from the file system
/// root, one name at a time, following no symbolic link: a resolved path
/// has none, so a link found here was put there after the grant resolved.
/// With `create`, each missing directory on the way is made.
fn walk(path: &Path, create: bool) -> io::Result<Dir> {
    let mut root = PathBuf::new();
    let mut names = Vec::new();
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => root.push(c),
            Component::Normal(name) => names.push(name),
            Component::CurDir => {}
            Component::ParentDir => return Err(escape()),
        }
    }
    if root.as_os_str().is_empty() {
        root.push(".");
    }
    let mut dir = Dir::open_ambient_dir(&root, ambient_authority())?;
    for name in names {
        dir = match dir.open_dir_nofollow(name) {
            Ok(next) => next,
            Err(error) if create && error.kind() == ErrorKind::NotFound => {
                match dir.create_dir(name) {
                    Err(error) if error.kind() != ErrorKind::AlreadyExists => return Err(error),
                    _ => {}
                }
                dir.open_dir_nofollow(name)
                    .map_err(|error| linked(&dir, name, error))?
            }
            Err(error) => return Err(linked(&dir, name, error)),
        };
    }
    Ok(dir)
}

/// `error`, or a refusal when `name` in `dir` is a symbolic link.
fn linked(dir: &Dir, name: &OsStr, error: io::Error) -> io::Error {
    if dir
        .symlink_metadata(name)
        .is_ok_and(|st| st.file_type().is_symlink())
    {
        escape()
    } else {
        error
    }
}

impl Place {
    /// The file opened for reading, or for writing (created, and truncated
    /// unless `append`).
    pub(crate) fn open(&self, write: bool, append: bool) -> io::Result<std::fs::File> {
        match self {
            Place::Ambient(path) => {
                let mut options = std::fs::OpenOptions::new();
                if write {
                    options
                        .write(true)
                        .create(true)
                        .truncate(!append)
                        .append(append);
                } else {
                    options.read(true);
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
                    // libuv opens directories too; a read then returns
                    // EISDIR, rather than failing the open with Windows'
                    // ERROR_ACCESS_DENIED.
                    options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
                }
                options.open(path)
            }
            Place::Under {
                dir, rel, entry, ..
            } => {
                let mut options = cap_std::fs::OpenOptions::new();
                if write {
                    options
                        .write(true)
                        .create(true)
                        .truncate(!append)
                        .append(append);
                } else {
                    options.read(true);
                }
                #[cfg(windows)]
                {
                    use cap_std::fs::OpenOptionsExt;
                    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
                    options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
                }
                if *entry {
                    options.follow(FollowSymlinks::No);
                }
                dir.open_with(rel, &options)
                    .map(cap_std::fs::File::into_std)
                    .map_err(|error| self.link_refused(error))
            }
            Place::Unreachable(error) => Err(again(error)),
        }
    }

    /// What is at the place, following a final symbolic link, but never
    /// the one at a granted entry's own name.
    pub(crate) fn metadata(&self) -> io::Result<Stat> {
        match self {
            Place::Ambient(path) => std::fs::metadata(path).map(Stat::from),
            Place::Under {
                dir,
                rel,
                entry: true,
                ..
            } => dir.symlink_metadata(rel).map(Stat::from),
            Place::Under { dir, rel, .. } => dir.metadata(rel).map(Stat::from),
            Place::Unreachable(error) => Err(again(error)),
        }
    }

    /// What is at the place, not following a final symbolic link.
    pub(crate) fn symlink_metadata(&self) -> io::Result<Stat> {
        match self {
            Place::Ambient(path) => std::fs::symlink_metadata(path).map(Stat::from),
            Place::Under { dir, rel, .. } => dir.symlink_metadata(rel).map(Stat::from),
            Place::Unreachable(error) => Err(again(error)),
        }
    }

    /// The directory's entry names, and the place of the directory, under
    /// which `child` names each entry.
    pub(crate) fn read_dir(&self) -> io::Result<(Place, Names)> {
        match self {
            Place::Ambient(path) => Ok((
                Place::Ambient(path.clone()),
                Box::new(std::fs::read_dir(path)?.map(|item| item.map(|e| e.file_name()))),
            )),
            Place::Under {
                dir,
                rel,
                entry,
                target,
            } => {
                let opened = if *entry {
                    dir.open_dir_nofollow(rel)
                } else {
                    dir.open_dir(rel)
                }
                .map_err(|error| self.link_refused(error))?;
                let names = opened.entries()?.map(|item| item.map(|e| e.file_name()));
                Ok((
                    Place::Under {
                        dir: Arc::new(opened),
                        rel: PathBuf::new(),
                        entry: false,
                        target: target.clone(),
                    },
                    Box::new(names),
                ))
            }
            Place::Unreachable(error) => Err(again(error)),
        }
    }

    /// The entry `name` of this directory.
    pub(crate) fn child(&self, name: &OsStr) -> Place {
        match self {
            Place::Ambient(path) => Place::Ambient(path.join(name)),
            Place::Under {
                dir, rel, target, ..
            } => Place::Under {
                dir: Arc::clone(dir),
                rel: rel.join(name),
                entry: false,
                target: target.join(name),
            },
            Place::Unreachable(error) => Place::Unreachable(again(error)),
        }
    }

    /// The directory that holds this place, while it is under the same
    /// handle: what a recursive `create_dir_all` makes first.
    pub(crate) fn parent(&self) -> Option<Place> {
        match self {
            Place::Ambient(path) => path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| Place::Ambient(p.to_owned())),
            Place::Under {
                dir,
                rel,
                entry: false,
                target,
            } => rel
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| Place::Under {
                    dir: Arc::clone(dir),
                    rel: p.to_owned(),
                    entry: false,
                    target: target.parent().unwrap_or(target).to_owned(),
                }),
            Place::Under { .. } | Place::Unreachable(_) => None,
        }
    }

    pub(crate) fn create_dir(&self) -> io::Result<()> {
        match self {
            Place::Ambient(path) => std::fs::create_dir(path),
            Place::Under { dir, rel, .. } => dir.create_dir(rel),
            Place::Unreachable(error) => Err(again(error)),
        }
    }

    pub(crate) fn remove_file(&self) -> io::Result<()> {
        match self {
            Place::Ambient(path) => std::fs::remove_file(path),
            Place::Under { dir, rel, .. } => dir.remove_file(rel),
            Place::Unreachable(error) => Err(again(error)),
        }
    }

    pub(crate) fn remove_dir(&self) -> io::Result<()> {
        match self {
            Place::Ambient(path) => std::fs::remove_dir(path),
            Place::Under { dir, rel, .. } => dir.remove_dir(rel),
            Place::Unreachable(error) => Err(again(error)),
        }
    }

    /// Renames this place to `to`; both are under the `FsWrite` grant, so
    /// both act by name or both under handles.
    pub(crate) fn rename(&self, to: &Place) -> io::Result<()> {
        match (self, to) {
            (Place::Ambient(from), Place::Ambient(to)) => std::fs::rename(from, to),
            (
                Place::Under { dir, rel, .. },
                Place::Under {
                    dir: to_dir,
                    rel: to_rel,
                    ..
                },
            ) => dir.rename(rel, to_dir, to_rel),
            (Place::Unreachable(error), _) | (_, Place::Unreachable(error)) => Err(again(error)),
            _ => Err(escape()),
        }
    }

    /// Whether opening the place can wait on another process: a FIFO, a
    /// device, a socket, or a Windows named pipe. A regular file, a
    /// directory, and a path that does not exist or cannot be inspected
    /// are not (the open reports the error at once).
    pub(crate) fn is_special(&self) -> bool {
        // std cannot tell a Windows pipe from a file, and inspecting a pipe
        // opens it, so this goes by the name.
        #[cfg(windows)]
        if let Place::Ambient(path) = self
            && super::io::device_path(&path.to_string_lossy())
        {
            return true;
        }
        self.metadata().is_ok_and(|st| st.special)
    }

    /// `error`, or a refusal when a granted entry's own name is now a
    /// symbolic link, which the open did not follow.
    fn link_refused(&self, error: io::Error) -> io::Error {
        match self {
            Place::Under {
                dir,
                rel,
                entry: true,
                ..
            } if dir
                .symlink_metadata(rel)
                .is_ok_and(|st| st.file_type().is_symlink()) =>
            {
                escape()
            }
            _ => error,
        }
    }
}

/// Windows reports a path below a file as "path not found", where Unix and
/// Node report `ENOTDIR` and the spec's `NotADirectory` (the rule
/// `std-fs.memory.error.not-directory`). When the open found nothing and
/// the nearest existing ancestor is not a directory, say so.
#[cfg(windows)]
pub(crate) fn below_a_file(place: &Place, error: io::Error) -> io::Error {
    let ancestor_not_dir = match place {
        _ if error.kind() != ErrorKind::NotFound => false,
        Place::Ambient(path) => path
            .ancestors()
            .skip(1)
            .find_map(|a| std::fs::metadata(a).ok())
            .is_some_and(|st| !st.is_dir()),
        Place::Under { dir, rel, .. } => rel
            .ancestors()
            .skip(1)
            .filter(|a| !a.as_os_str().is_empty())
            .find_map(|a| dir.metadata(a).ok())
            .is_some_and(|st| !st.is_dir()),
        Place::Unreachable(_) => false,
    };
    if ancestor_not_dir {
        ErrorKind::NotADirectory.into()
    } else {
        error
    }
}

#[cfg(not(windows))]
pub(crate) fn below_a_file(_: &Place, error: io::Error) -> io::Error {
    error
}

/// A directory's entry names as they are read.
pub(crate) type Names = Box<dyn Iterator<Item = io::Result<std::ffi::OsString>>>;
