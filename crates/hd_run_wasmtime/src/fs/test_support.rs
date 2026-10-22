use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use hd_run::Grants;

use crate::codec::{Enc, Wire};

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

/// The trait key of the `Fs` method `method`.
pub(crate) fn key(method: &str) -> &'static str {
    crate::abi::METHODS
        .iter()
        .find(|m| m.name == method && m.module.starts_with("hd:Fs"))
        .and_then(|m| m.module.strip_prefix("hd:"))
        .expect("an Fs method")
}

/// The encoded `.Err` of the `FsError` variant `variant` with its one
/// field, `text`.
pub(crate) fn error(text: &str, variant: u64) -> Enc {
    let mut e = Enc::default();
    e.leb(1).leb(variant).str(text);
    e
}

pub(crate) fn invoke(method: &str, path: &str, payload: Option<&[u8]>) -> Enc {
    let mut args = Enc::default();
    args.str(path);
    if let Some(bytes) = payload {
        args.bytes(bytes);
    }
    call(key(method), method, args.done(), &Grants::default()).expect("host call")
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
        key(method),
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

/// The modification timestamp appended to an Entry, from the same
/// filesystem metadata as the provider, without changing the file.
pub(crate) fn modified(e: &mut Enc, path: impl AsRef<std::path::Path>) {
    let time = fs::symlink_metadata(path).unwrap().modified().unwrap();
    let millis = i64::try_from(
        time.duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    e.leb(1);
    millis.enc(e);
}
