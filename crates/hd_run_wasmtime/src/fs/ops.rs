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
pub(crate) struct Env<'a> {
    pub(crate) cwd: &'a Path,
    pub(crate) deadline: Option<Instant>,
    /// Set for a call that runs inline: a `remove` or a `rename` first
    /// drops the handles of granted directories it takes away, which
    /// Windows would not let it remove while they are open.
    pub(crate) sandbox: Option<&'a Sandbox>,
}

/// Runs `method` at `place`, the sandbox's place for the guest's `path`
/// (`to` for the destination of a `rename`), and gives its encoded result.
pub(crate) fn run(
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

#[cfg(test)]
mod tests {
    use crate::codec::Enc;
    use crate::fs::test_support::{TempDir, error, invoke};
    use std::fs;

    #[test]
    fn reads_text_strictly_and_preserves_every_bom() {
        let dir = TempDir::new();
        let path = dir.path("text");
        fs::write(&path, "\u{feff}\u{feff}hé").unwrap();
        let actual = invoke("read_text", &path, None);
        let mut expected = Enc::default();
        expected.leb(0).str("\u{feff}\u{feff}hé");
        assert_eq!(actual.done(), expected.done());
        for bytes in [&[0xc0, 0xaf][..], &[0xed, 0xa0, 0x80], &[0xe2, 0x82]] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(
                invoke("read_text", &path, None).done(),
                error(&path, 6).done()
            );
            let mut expected = Enc::default();
            expected.leb(0).bytes(bytes);
            assert_eq!(invoke("read_bytes", &path, None).done(), expected.done());
        }
    }
    #[cfg(unix)]
    #[test]
    fn rejects_oversized_regular_files_before_reading_them() {
        let dir = TempDir::new();
        let path = dir.path("sparse");
        fs::File::create(&path)
            .unwrap()
            .set_len(0x8000_0000)
            .unwrap();
        let expected = error("File size (2147483648) is greater than 2 GiB", 7);
        for method in ["read_text", "read_bytes"] {
            assert_eq!(invoke(method, &path, None).done(), expected.done());
        }
    }
    /// A directory used as a file is `IsADirectory` on every OS, whatever
    /// code the OS gives (Windows: "Incorrect function" on read, "The
    /// parameter is incorrect" on a write-open).
    #[test]
    fn a_directory_read_or_written_as_a_file_is_a_directory_error() {
        let dir = TempDir::new();
        let path = dir.0.to_string_lossy().into_owned();
        for method in ["read_text", "read_bytes"] {
            assert_eq!(
                invoke(method, &path, None).done(),
                error(&path, 5).done(),
                "{method}"
            );
        }
        for method in ["write_bytes", "write_text", "append_text"] {
            assert_eq!(
                invoke(method, &path, Some(b"no")).done(),
                error(&path, 5).done(),
                "{method}"
            );
        }
    }
    /// A path below a file is `NotADirectory` on every OS, as Node reports
    /// it (Windows says "path not found" for the open).
    #[test]
    fn a_path_below_a_file_is_a_not_a_directory_error() {
        let dir = TempDir::new();
        let file = dir.path("file");
        fs::write(&file, "hello").unwrap();
        let below = format!("{file}/child");
        for method in ["read_bytes", "read_text"] {
            assert_eq!(
                invoke(method, &below, None).done(),
                error(&below, 4).done(),
                "{method}"
            );
        }
        assert_eq!(
            invoke("write_text", &below, Some(b"no")).done(),
            error(&below, 4).done()
        );
        assert_eq!(
            invoke("list_dir", &below, None).done(),
            error(&below, 4).done()
        );
    }
    #[test]
    fn recursive_mkdir_and_stat_have_node_edge_behavior() {
        let dir = TempDir::new();
        let path = dir.path("a/b/c");
        assert_eq!(invoke("create_dir_all", &path, None).done(), &[0]);
        assert_eq!(invoke("create_dir_all", &path, None).done(), &[0]);
        assert_eq!(
            invoke("create_dir_all", "", None).done(),
            error("", 0).done()
        );
        let file = dir.path("file");
        fs::write(&file, "hello").unwrap();
        assert_eq!(
            invoke("create_dir_all", &file, None).done(),
            error(&file, 4).done()
        );
        assert_eq!(
            invoke("stat", &format!("{file}/child"), None).done(),
            &[0, 0]
        );
        assert_eq!(invoke("stat", &dir.path("absent"), None).done(), &[0, 0]);
    }
    #[cfg(unix)]
    #[test]
    fn lists_byte_order_and_reports_symlink_and_directory_sizes() {
        use crate::codec::Dec;
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        fs::write(dir.path("é"), "hello").unwrap();
        fs::write(dir.path("z"), "z").unwrap();
        fs::create_dir(dir.path("a")).unwrap();
        symlink("é", dir.path("link")).unwrap();
        let path = dir.0.to_string_lossy();
        let actual = invoke("list_dir", &path, None);
        let mut d = Dec::new(actual.done());
        assert_eq!(d.leb().unwrap(), 0);
        assert_eq!(d.leb().unwrap(), 4);
        for (name, kind, size) in [("a", 1, 0), ("link", 2, 2), ("z", 0, 1), ("é", 0, 5)] {
            assert_eq!(d.str().unwrap(), format!("{path}/{name}"));
            assert_eq!(d.leb().unwrap(), kind);
            assert_eq!(d.leb().unwrap(), size);
        }
        let link = dir.path("link");
        let mut expected = Enc::default();
        expected.leb(0).leb(1).str(&link).leb(2).leb(2);
        assert_eq!(invoke("stat", &link, None).done(), expected.done());
        assert_eq!(invoke("remove", &link, None).done(), &[0]);
        assert!(fs::metadata(dir.path("é")).is_ok());
        let dangling = dir.path("dangling");
        symlink("missing", &dangling).unwrap();
        assert_eq!(
            invoke("create_dir_all", &dangling, None).done(),
            error(&dangling, 0).done()
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn write_errors_keep_the_native_message_and_path() {
        let native = std::io::Error::from_raw_os_error(rustix::io::Errno::NOSPC.raw_os_error());
        assert_eq!(
            invoke("write_text", "/dev/full", Some(b"x")).done(),
            error(&format!("write \"/dev/full\": {native}"), 7).done()
        );
    }
}
