//! Special files: a FIFO makes its call a job for the blocking pool
//! (`std-fs.special-file`), and everything else runs inline.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use hd_run::Grants;

use super::api::{Job, Start, start};
use super::test_support::TempDir;
use crate::codec::{Dec, Enc};
use crate::limits::{Ledger, is_heap_exhausted};

fn fifo(dir: &TempDir) -> String {
    let path = dir.path("fifo");
    crate::test_fifo::make(Path::new(&path));
    path
}

fn begin(method: &str, path: &str, payload: Option<&[u8]>) -> Start {
    begin_with(&Ledger::new(u64::MAX), method, path, payload).expect("start")
}

fn begin_with(
    ledger: &Arc<Ledger>,
    method: &str,
    path: &str,
    payload: Option<&[u8]>,
) -> wasmtime::Result<Start> {
    let mut args = Enc::default();
    args.str(path);
    if let Some(bytes) = payload {
        args.bytes(bytes);
    }
    start(
        super::test_support::key(method),
        method,
        args.done(),
        &Grants::default(),
        &super::Sandbox::default(),
        None,
        &std::env::current_dir().unwrap(),
        ledger,
    )
}

fn job(method: &str, path: &str, payload: Option<&[u8]>) -> Box<Job> {
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
    let (encoded, _held) = reader.join().unwrap().expect("within the budget");
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
    assert_eq!(
        writer.join().unwrap().expect("within the budget").0.done(),
        &[0]
    );
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
    // The lingering thread must end, and dropping the runtime waits for it.
    // A writer that opens and closes once can come before the reader's
    // `open` returns, which leaves the reader in `read` for good on macOS.
    // So a helper keeps opening and closing an end that never blocks until
    // the thread is gone; each close ends the read of a reader that is in.
    let finished = Arc::new(AtomicBool::new(false));
    let helper = {
        let (path, finished) = (path.clone(), finished.clone());
        std::thread::spawn(move || {
            while !finished.load(Ordering::SeqCst) {
                let end = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .unwrap();
                std::thread::sleep(Duration::from_millis(20));
                drop(end);
                std::thread::sleep(Duration::from_millis(5));
            }
        })
    };
    drop(runtime);
    finished.store(true, Ordering::SeqCst);
    helper.join().unwrap();
}

/// A job on `ledger`, as `.start` makes it.
fn job_on(ledger: &Arc<Ledger>, method: &str, path: &str, payload: Option<&[u8]>) -> Box<Job> {
    match begin_with(ledger, method, path, payload).expect("start") {
        Start::Run(job) => job,
        Start::Ready(_) => panic!("{method} of a FIFO ran inline"),
    }
}

/// `std-host.op.heap.exhausted`: a FIFO that streams more than the budget
/// ends the read at the budget, where it would otherwise grow without
/// bound. The bytes it reserved are released with the job.
#[test]
fn a_fifo_read_past_the_heap_budget_is_exhausted() {
    let dir = TempDir::new();
    let path = fifo(&dir);
    let ledger = Ledger::new(100_000);
    let reading = job_on(&ledger, "read_bytes", &path, None);
    let reader = std::thread::spawn(move || reading.run_blocking());
    let mut writer = release(&path, true);
    // The reader stops at the budget and closes its end: the write ends in
    // a broken pipe, never in 10 MB held.
    let sent = writer.write_all(&vec![7; 10_000_000]);
    assert!(sent.is_err(), "the reader went on past the budget");
    drop(writer);
    let Err(error) = reader.join().unwrap() else {
        panic!("the read passed the heap budget");
    };
    assert!(is_heap_exhausted(&error.into()));
    assert_eq!(ledger.charged(), 0, "the failed read kept a reservation");
}

/// A FIFO read within the budget holds what it read until `.finish`
/// drops the reservation, and the ledger returns to zero.
#[test]
fn a_fifo_read_holds_its_bytes_until_finish() {
    let dir = TempDir::new();
    let path = fifo(&dir);
    let ledger = Ledger::new(1_000_000);
    let reading = job_on(&ledger, "read_bytes", &path, None);
    let reader = std::thread::spawn(move || reading.run_blocking());
    let mut writer = release(&path, true);
    writer.write_all(&vec![7; 300_000]).unwrap();
    drop(writer);
    let (_, held) = reader.join().unwrap().expect("within the budget");
    assert_eq!(ledger.charged(), 300_000);
    drop(held);
    assert_eq!(ledger.charged(), 0);
}

/// A write's payload is reserved at `.start`, and a payload past the
/// budget panics there, before the FIFO is opened.
#[test]
fn a_fifo_write_reserves_its_payload_at_start() {
    let dir = TempDir::new();
    let path = fifo(&dir);
    let ledger = Ledger::new(10);
    let Err(error) = begin_with(&ledger, "write_bytes", &path, Some(&[0; 11])) else {
        panic!("a payload past the budget started");
    };
    assert!(is_heap_exhausted(&error));
    assert_eq!(ledger.charged(), 0);
    let writing = job_on(&ledger, "write_bytes", &path, Some(&[1; 10]));
    assert_eq!(ledger.charged(), 10);
    let writer = std::thread::spawn(move || writing.run_blocking());
    let mut bytes = Vec::new();
    release(&path, false).read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, [1; 10]);
    let (_, held) = writer.join().unwrap().expect("within the budget");
    drop(held);
    assert_eq!(ledger.charged(), 0);
}

/// An inline regular file holds nothing: its data goes straight to the
/// program's memory, which the budget already counts.
#[test]
fn a_regular_file_is_not_charged() {
    let dir = TempDir::new();
    let file = dir.path("file");
    fs::write(&file, vec![1; 1000]).unwrap();
    let ledger = Ledger::new(0);
    let Start::Ready(read) = begin_with(&ledger, "read_bytes", &file, None).unwrap() else {
        panic!("a regular file was offloaded");
    };
    assert_eq!(Dec::new(read.done()).leb().unwrap(), 0);
    let Start::Ready(wrote) = begin_with(&ledger, "write_bytes", &file, Some(&[2; 1000])).unwrap()
    else {
        panic!("a regular file was offloaded");
    };
    assert_eq!(wrote.done(), &[0]);
    assert_eq!(ledger.charged(), 0);
}
