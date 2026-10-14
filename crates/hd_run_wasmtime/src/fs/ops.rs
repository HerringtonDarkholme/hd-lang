use std::fs;
use std::io;
use std::path::Path;
use std::time::Instant;

use crate::codec::Enc;

use super::api::{Failure, check_deadline, entry, error};
use super::io::{open, read, write};

pub(crate) fn run(
    method: &str,
    path: &str,
    to: Option<&str>,
    payload: Option<&[u8]>,
    e: &mut Enc,
    deadline: Option<Instant>,
    cwd: &Path,
) -> Result<(), Failure> {
    check_deadline(deadline)?;
    // Keep an empty argument empty: Node rejects it instead of treating it
    // as the working directory. Absolute paths are unchanged by join.
    let at = |p: &str| {
        if p.is_empty() {
            std::path::PathBuf::new()
        } else {
            cwd.join(p)
        }
    };
    let actual = at(path);
    match method {
        "read_bytes" => {
            e.leb(0).bytes(&read(&actual, deadline)?);
        }
        "read_text" => {
            let bytes = read(&actual, deadline)?;
            check_deadline(deadline)?;
            if let Ok(text) = std::str::from_utf8(&bytes) {
                e.leb(0).str(text);
            } else {
                *e = error(path, 6);
            }
        }
        "stat" => match fs::symlink_metadata(&actual) {
            Ok(st) => {
                entry(e.leb(0).leb(1), path, &st);
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                e.leb(0).leb(0);
            }
            Err(err) => return Err(Failure::new("lstat", err)),
        },
        "list_dir" => {
            #[cfg(windows)]
            if fs::metadata(&actual).is_ok_and(|st| !st.is_dir()) {
                return Err(Failure::new("scandir", io::ErrorKind::NotADirectory.into()));
            }
            // Incremental heap ordering allows checks during ordering, not just
            // around one potentially large, uncancellable sort operation.
            let mut names = std::collections::BinaryHeap::new();
            for item in fs::read_dir(&actual).map_err(|err| Failure::new("scandir", err))? {
                check_deadline(deadline)?;
                let item = item.map_err(|err| Failure::new("scandir", err))?;
                names.push(std::cmp::Reverse(
                    item.file_name().to_string_lossy().into_owned(),
                ));
            }
            let under = if path.ends_with('/') {
                path.to_owned()
            } else {
                format!("{path}/")
            };
            let mut found = Vec::new();
            while let Some(std::cmp::Reverse(n)) = names.pop() {
                check_deadline(deadline)?;
                let p = format!("{under}{n}");
                if let Ok(st) = fs::symlink_metadata(actual.join(&n)) {
                    found.push((p, st));
                }
            }
            e.leb(0).leb(u64::try_from(found.len()).unwrap_or(u64::MAX));
            for (p, st) in found {
                check_deadline(deadline)?;
                entry(e, &p, &st);
            }
        }
        "write_bytes" | "write_text" => {
            let mut file = open(&actual, true, false, deadline)?;
            write(&mut file, payload.unwrap_or_default(), deadline)?;
            e.leb(0);
        }
        "append_text" => {
            let mut file = open(&actual, true, true, deadline)?;
            write(&mut file, payload.unwrap_or_default(), deadline)?;
            e.leb(0);
        }
        "create_dir_all" => {
            if fs::metadata(&actual).is_ok_and(|st| !st.is_dir()) {
                *e = error(path, 4);
            } else {
                if path.is_empty() {
                    // std accepts the empty path; Node mkdir does not.
                    *e = error(path, 0);
                    return Ok(());
                }
                mkdir(&actual, deadline).map_err(|err| shown_mkdir_failure(err, cwd, path))?;
                e.leb(0);
            }
        }
        "remove" => {
            if fs::symlink_metadata(&actual)
                .map_err(|err| Failure::new("lstat", err))?
                .is_dir()
            {
                check_deadline(deadline)?;
                fs::remove_dir(&actual).map_err(|err| Failure::new("rmdir", err))?;
            } else {
                check_deadline(deadline)?;
                fs::remove_file(&actual).map_err(|err| Failure::new("unlink", err))?;
            }
            e.leb(0);
        }
        "rename" => {
            fs::rename(&actual, at(to.unwrap_or_default()))
                .map_err(|err| Failure::new("rename", err))?;
            e.leb(0);
        }
        _ => unreachable!("registered filesystem method"),
    }
    Ok(())
}

fn mkdir(path: &Path, deadline: Option<Instant>) -> Result<(), Failure> {
    check_deadline(deadline)?;
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                mkdir(parent, deadline)?;
                check_deadline(deadline)?;
                // Retry after the parent exists, including a race with another creator.
                return mkdir_finish(path, fs::create_dir(path));
            }
            Err(mkdir_failure(path, err))
        }
        result => mkdir_finish(path, result),
    }
}

fn mkdir_finish(path: &Path, result: io::Result<()>) -> Result<(), Failure> {
    match result {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => match fs::metadata(path) {
            Ok(st) if st.is_dir() => Ok(()),
            Ok(_) => Err(mkdir_failure(path, err)),
            // Node's recursive mkdir follows an existing symlink: a dangling
            // target yields ENOENT, and a loop yields ELOOP, rather than EEXIST.
            Err(err) => Err(mkdir_failure(path, err)),
        },
        Err(err) => Err(mkdir_failure(path, err)),
    }
}

fn mkdir_failure(path: &Path, error: io::Error) -> Failure {
    Failure::System {
        error,
        syscall: "mkdir",
        path: Some(path.to_string_lossy().into_owned()),
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
    use crate::fs::api::error;
    use crate::fs::test_support::{TempDir, invoke};
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
        let native = std::io::Error::from_raw_os_error(libc::ENOSPC);
        assert_eq!(
            invoke("write_text", "/dev/full", Some(b"x")).done(),
            error(&format!("write \"/dev/full\": {native}"), 7).done()
        );
    }
}
