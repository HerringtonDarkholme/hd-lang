use std::io;
use std::path::Path;
use std::time::Instant;

use crate::abi::{self, Entry, FsError};
use crate::codec::Enc;

use super::api::{Failure, check_deadline, entry, error, ok};
use super::io::{open, read, write};
use super::sandbox::{Place, Sandbox, below_a_file};

/// A guest path against the case's directory. An empty argument stays
/// empty: Node rejects it instead of treating it as the working directory.
/// Absolute paths are unchanged by join.
pub(crate) fn at(cwd: &Path, path: &str) -> std::path::PathBuf {
    if path.is_empty() {
        std::path::PathBuf::new()
    } else {
        cwd.join(path)
    }
}

/// Where and under what limits a call runs.
#[derive(Clone, Copy)]
pub struct Env<'a> {
    pub cwd: &'a Path,
    pub deadline: Option<Instant>,
    /// Set for a call that runs inline: a `remove` or a `rename` first
    /// drops the handles of granted directories it takes away, which
    /// Windows would not let it remove while they are open.
    pub sandbox: Option<&'a Sandbox>,
}

/// Runs `method` at `place`, the sandbox's place for the guest's `path`
/// (`to` for the destination of a `rename`), and gives its encoded result.
pub fn run(
    method: &str,
    path: &str,
    place: &Place,
    to: Option<&Place>,
    payload: Option<&[u8]>,
    env: Env<'_>,
) -> Result<Enc, Failure> {
    let Env {
        cwd,
        deadline,
        sandbox,
    } = env;
    let path_of = || abi::Path(path.to_owned());
    check_deadline(deadline)?;
    Ok(match method {
        "read_bytes" => ok(read(place, deadline)?),
        "read_text" => {
            let bytes = read(place, deadline)?;
            check_deadline(deadline)?;
            match String::from_utf8(bytes) {
                Ok(text) => ok(text),
                Err(_) => error(FsError::InvalidUtf8 { path: path_of() }),
            }
        }
        "stat" => match place.symlink_metadata() {
            Ok(st) => ok(Some(entry(path, &st))),
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                ok(None::<Entry>)
            }
            Err(err) => return Err(Failure::new("lstat", err)),
        },
        "list_dir" => {
            #[cfg(windows)]
            if place.metadata().is_ok_and(|st| !st.dir) {
                return Err(Failure::new("scandir", io::ErrorKind::NotADirectory.into()));
            }
            // Incremental heap ordering allows checks during ordering, not just
            // around one potentially large, uncancellable sort operation.
            let mut names = std::collections::BinaryHeap::new();
            // cost: the running program's own listing, one per its `scandir` call.
            let (dir, entries) = place
                .read_dir()
                .map_err(|err| Failure::new("scandir", below_a_file(place, err)))?;
            for item in entries {
                check_deadline(deadline)?;
                let name = item.map_err(|err| Failure::new("scandir", err))?;
                names.push(std::cmp::Reverse((
                    name.to_string_lossy().into_owned(),
                    name,
                )));
            }
            let under = if path.ends_with('/') {
                path.to_owned()
            } else {
                format!("{path}/")
            };
            let mut found = Vec::new();
            while let Some(std::cmp::Reverse((shown, name))) = names.pop() {
                check_deadline(deadline)?;
                if let Ok(st) = dir.child(&name).symlink_metadata() {
                    found.push(entry(&format!("{under}{shown}"), &st));
                }
            }
            ok(found)
        }
        "write_bytes" | "write_text" => {
            let mut file = open(place, true, false, deadline)?;
            write(&mut file, payload.unwrap_or_default(), deadline)?;
            ok(())
        }
        "append_text" => {
            let mut file = open(place, true, true, deadline)?;
            write(&mut file, payload.unwrap_or_default(), deadline)?;
            ok(())
        }
        "create_dir_all" => {
            if place.metadata().is_ok_and(|st| !st.dir) {
                error(FsError::NotADirectory { path: path_of() })
            } else if path.is_empty() {
                // std accepts the empty path; Node mkdir does not.
                error(FsError::NotFound { path: path_of() })
            } else {
                mkdir(place, &at(cwd, path), deadline)
                    .map_err(|err| shown_mkdir_failure(err, cwd, path))?;
                ok(())
            }
        }
        "remove" => {
            let dir = place
                .symlink_metadata()
                .map_err(|err| Failure::new("lstat", err))?
                .dir;
            check_deadline(deadline)?;
            if let Some(sandbox) = sandbox {
                sandbox.forget(place);
            }
            if dir {
                place
                    .remove_dir()
                    .map_err(|err| Failure::new("rmdir", err))?;
            } else {
                place
                    .remove_file()
                    .map_err(|err| Failure::new("unlink", err))?;
            }
            ok(())
        }
        "rename" => {
            let to = to.ok_or_else(|| Failure::Message("a rename has no destination".into()))?;
            if let Some(sandbox) = sandbox {
                sandbox.forget(place);
                sandbox.forget(to);
            }
            place
                .rename(to)
                .map_err(|err| Failure::new("rename", err))?;
            ok(())
        }
        _ => unreachable!("registered filesystem method"),
    })
}

/// Makes the directory at `place` and its missing parents. `shown` is the
/// guest's path at the same depth, which an error names.
fn mkdir(place: &Place, shown: &Path, deadline: Option<Instant>) -> Result<(), Failure> {
    check_deadline(deadline)?;
    match place.create_dir() {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = place.parent() {
                mkdir(&parent, shown.parent().unwrap_or(shown), deadline)?;
                check_deadline(deadline)?;
                // Retry after the parent exists, including a race with another creator.
                return mkdir_finish(place, shown, place.create_dir());
            }
            Err(mkdir_failure(shown, err))
        }
        result => mkdir_finish(place, shown, result),
    }
}

fn mkdir_finish(place: &Place, shown: &Path, result: io::Result<()>) -> Result<(), Failure> {
    match result {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => match place.metadata() {
            Ok(st) if st.dir => Ok(()),
            Ok(_) => Err(mkdir_failure(shown, err)),
            // Node's recursive mkdir follows an existing symlink: a dangling
            // target yields ENOENT, and a loop yields ELOOP, rather than EEXIST.
            Err(err) => Err(mkdir_failure(shown, err)),
        },
        Err(err) => Err(mkdir_failure(shown, err)),
    }
}

fn mkdir_failure(path: &Path, error: io::Error) -> Failure {
    match Failure::new("mkdir", error) {
        Failure::System { error, syscall, .. } => Failure::System {
            error,
            syscall,
            path: Some(path.to_string_lossy().into_owned()),
        },
        refused => refused,
    }
}

fn shown_mkdir_failure(failure: Failure, cwd: &Path, original: &str) -> Failure {
    match failure {
        Failure::System {
            error,
            syscall,
            path: Some(path),
        } if Path::new(original).is_relative() => {
            let failed = Path::new(&path);
            let shown = if failed == cwd.join(original) {
                original.to_owned()
            } else {
                failed
                    .strip_prefix(cwd)
                    .unwrap_or(failed)
                    .to_string_lossy()
                    .into_owned()
            };
            Failure::System {
                error,
                syscall,
                path: Some(shown),
            }
        }
        failure => failure,
    }
}
