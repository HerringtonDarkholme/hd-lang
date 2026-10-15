//! Special files: a FIFO makes its call a job for the blocking pool
//! (`std-fs.special-file`), and everything else runs inline.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use hd_run::Grants;

use super::api::{Job, Start, start};
use super::test_support::TempDir;
use crate::codec::{Dec, Enc};

fn fifo(dir: &TempDir) -> String {
    let path = dir.path("fifo");
    crate::test_fifo::make(Path::new(&path));
    path
}

fn begin(method: &str, path: &str, payload: Option<&[u8]>) -> Start {
    let mut args = Enc::default();
    args.str(path);
    if let Some(bytes) = payload {
        args.bytes(bytes);
    }
    let key = if super::READ.contains(&method) {
        "FsRead"
    } else {
        "FsWrite"
    };
    start(
        key,
        method,
        args.done(),
        &Grants::default(),
        None,
        &std::env::current_dir().unwrap(),
    )
    .expect("start")
}

fn job(method: &str, path: &str, payload: Option<&[u8]>) -> Job {
    match begin(method, path, payload) {
        Start::Run(job) => job,
        Start::Ready(_) => panic!("{method} of a FIFO ran inline"),
    }
}

fn ready(method: &str, path: &str, payload: Option<&[u8]>) -> Enc {
    match begin(method, path, payload) {
        Start::Ready(encoded) => encoded,
        Start::Run(_) => panic!("{method} of {path} was offloaded"),
    }
}

/// Opens the FIFO's other end, so that a call waiting on it goes on.
fn release(path: &str, write: bool) -> fs::File {
    fs::OpenOptions::new()
        .read(!write)
        .write(write)
        .open(path)
        .unwrap()
}

#[test]
fn regular_files_directories_and_missing_paths_run_inline() {
    let dir = TempDir::new();
    let file = dir.path("file");
    fs::write(&file, b"data").unwrap();
    let absent = dir.path("absent");
    for (method, path, payload) in [
        ("read_bytes", file.as_str(), None),
        ("read_text", file.as_str(), None),
        ("write_bytes", file.as_str(), Some(b"x".as_slice())),
        ("append_text", file.as_str(), Some(b"y".as_slice())),
        ("read_bytes", absent.as_str(), None),
        ("write_text", absent.as_str(), Some(b"z".as_slice())),
        // A directory fails at the read, which is quick.
        ("read_bytes", dir.0.to_str().unwrap(), None),
        ("list_dir", dir.0.to_str().unwrap(), None),
    ] {
        ready(method, path, payload);
    }
    assert_eq!(fs::read(&file).unwrap(), b"xy");
    assert_eq!(fs::read(&absent).unwrap(), b"z");
}

#[test]
fn a_fifo_is_a_job_only_for_the_methods_that_open_it() {
    let dir = TempDir::new();
    let path = fifo(&dir);
    for (method, payload) in [
        ("read_bytes", None),
        ("read_text", None),
        ("write_bytes", Some(b"x".as_slice())),
        ("write_text", Some(b"x".as_slice())),
        ("append_text", Some(b"x".as_slice())),
    ] {
        job(method, &path, payload);
    }
    // These never open the FIFO: `stat` and `remove` look at the entry.
    let mut found = Dec::new(ready("stat", &path, None).done()).leb().unwrap();
    assert_eq!(found, 0);
    found = Dec::new(ready("remove", &path, None).done()).leb().unwrap();
    assert_eq!(found, 0);
    assert!(!Path::new(&path).exists());
}

#[test]
fn a_fifo_read_with_a_writer_reads_everything() {
    let dir = TempDir::new();
    let path = fifo(&dir);
    let payload: Vec<u8> = (0..=255u8).cycle().take(300_000).collect();
    let reading = job("read_bytes", &path, None);
    let reader = std::thread::spawn(move || reading.run_blocking());
    let mut writer = release(&path, true);
    writer.write_all(&payload).unwrap();
    drop(writer);
    let encoded = reader.join().unwrap();
    let mut dec = Dec::new(encoded.done());
    assert_eq!(dec.leb().unwrap(), 0);
    assert_eq!(dec.bytes().unwrap(), payload);
}

#[test]
fn a_fifo_write_waits_for_a_reader() {
    let dir = TempDir::new();
    let path = fifo(&dir);
    let writing = job("write_text", &path, Some(b"through the pipe"));
    let writer = std::thread::spawn(move || writing.run_blocking());
    let mut text = String::new();
    release(&path, false).read_to_string(&mut text).unwrap();
    assert_eq!(text, "through the pipe");
    assert_eq!(writer.join().unwrap().done(), &[0]);
}

#[test]
fn dropping_a_waiting_job_returns_at_once_and_leaves_the_thread() {
    let dir = TempDir::new();
    let path = fifo(&dir);
    let waiting = job("read_text", &path, None);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let started = Instant::now();
    let outcome = runtime
        .block_on(async { tokio::time::timeout(Duration::from_millis(100), waiting.run()).await });
    assert!(outcome.is_err(), "no writer has come, so the read waits");
    assert!(started.elapsed() < Duration::from_secs(2));
    // Let the lingering thread finish: a writer arrives and closes.
    drop(release(&path, true));
    drop(runtime);
}
