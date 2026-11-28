//! Atomic temporary directories, using tempfile's name/collision algorithm
//! with cap-std directory-relative creation and removal. The host retains a
//! fallback owner until the guest explicitly removes the directory, covering
//! cancellation before a created path reaches its `TempDir` guard.
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use hd_base::HashMap;
use hd_run::Grants;

use super::api::{self, Start};
use super::sandbox::{Place, Sandbox};
use crate::abi::FsError;

pub struct Directory {
    pub path: PathBuf,
    place: Place,
    armed: bool,
}

impl Drop for Directory {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.place.remove_dir_all();
        }
    }
}

#[derive(Default)]
pub(crate) struct Directories(HashMap<PathBuf, Directory>);

impl Directories {
    pub fn insert(&mut self, directory: Directory) -> String {
        let path = directory.path.to_string_lossy().into_owned();
        self.0.insert(directory.path.clone(), directory);
        path
    }

    pub fn forget(&mut self, path: &Path) {
        if let Some(mut directory) = self.0.remove(path) {
            directory.armed = false;
        }
    }
}

pub(crate) fn create(
    prefix: &str,
    grants: &Grants,
    sandbox: &Sandbox,
    deadline: Option<Instant>,
    cwd: &Path,
) -> wasmtime::Result<Start> {
    let root = hd_run::resolve_path(&std::env::temp_dir());
    if root.to_str().is_none() {
        return Ok(Start::Ready(api::error(FsError::Other {
            message: "temporary directory path is not valid UTF-8".into(),
        })));
    }
    if !grants.covers_path("FsWrite", &root, cwd) {
        return Ok(Start::Ready(api::error(FsError::NotGranted {
            path: crate::abi::Path(root.to_string_lossy().into_owned()),
        })));
    }
    if prefix.chars().any(std::path::is_separator) || prefix.contains('\0') {
        return Ok(Start::Ready(api::error(FsError::Other {
            message: "temporary-directory prefix contains a path separator or NUL".into(),
        })));
    }
    let Some(place) = sandbox.place(grants, "FsWrite", &root, true, false) else {
        return Ok(Start::Ready(api::error(FsError::NotGranted {
            path: crate::abi::Path(root.to_string_lossy().into_owned()),
        })));
    };
    // tempfile bounds collision attempts; each attempt makes one atomic mkdir
    // under the retained grant handle. No retry scans accumulated directories.
    let made = tempfile::Builder::new()
        .prefix(prefix)
        .disable_cleanup(true)
        .make_in(&root, |candidate| {
            api::check_deadline(deadline).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "temporary-directory creation timed out",
                )
            })?;
            let name = candidate
                .file_name()
                .ok_or_else(|| io::Error::other("temporary-directory candidate has no name"))?;
            let child = place.child(name);
            child.create_private_dir()?;
            Ok(Directory {
                path: candidate.to_owned(),
                place: child,
                armed: true,
            })
        });
    match made {
        Ok(made) => {
            let (directory, path) = made.into_parts();
            drop(path); // Name cleanup is disabled; Directory owns recursive cleanup.
            api::check_deadline(deadline).map_err(|_| crate::host::Stop::TimeLimit)?;
            Ok(Start::Temporary(directory))
        }
        Err(error) => Ok(Start::Ready(api::failure(
            &root.to_string_lossy(),
            "create_temp_dir",
            &root.to_string_lossy(),
            &api::Failure::new("mkdir", error),
        ))),
    }
}
