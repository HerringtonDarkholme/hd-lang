use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use hd_run::Grants;

use crate::codec::Enc;

use super::api::{call, call_until};

pub(crate) struct TempDir(pub(crate) PathBuf);

impl TempDir {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hd-fs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create test directory");
        Self(path)
    }

    pub(crate) fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn invoke(method: &str, path: &str, payload: Option<&[u8]>) -> Enc {
    let mut args = Enc::default();
    args.str(path);
    if let Some(bytes) = payload {
        args.bytes(bytes);
    }
    call(
        if super::READ.contains(&method) {
            "FsRead"
        } else {
            "FsWrite"
        },
        method,
        args.done(),
        &Grants::default(),
    )
    .expect("host call")
}

pub(crate) fn limited(
    method: &str,
    path: &str,
    payload: Option<&[u8]>,
    deadline: Instant,
) -> wasmtime::Result<Enc> {
    let mut args = Enc::default();
    args.str(path);
    if let Some(bytes) = payload {
        args.bytes(bytes);
    }
    call_until(
        if super::READ.contains(&method) {
            "FsRead"
        } else {
            "FsWrite"
        },
        method,
        args.done(),
        &Grants::default(),
        Some(deadline),
    )
}

pub(crate) fn assert_time_limit(result: wasmtime::Result<Enc>) {
    let Err(err) = result else {
        panic!("deadline should stop the host call");
    };
    assert!(
        matches!(
            err.downcast_ref::<crate::host::Stop>(),
            Some(crate::host::Stop::TimeLimit)
        ),
        "{err}"
    );
}
