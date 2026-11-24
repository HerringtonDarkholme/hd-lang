//! Opt-in, serial case attribution; normal conformance never enters this module.
//! LLVM owns the counter bank. A single worker resets/writes it only between
//! completed cases. The converter acknowledges deletion before the next write.

#![allow(
    unexpected_cfgs,
    reason = "cargo-llvm-cov supplies cfg(coverage) only to instrumented builds"
)]

use super::{Case, Store, Verdict};
use std::path::Path;

pub(super) fn run(cases: &[Case], store: &Store, directory: &Path) -> Vec<Verdict> {
    #[cfg(coverage)]
    {
        // cost: one worker on the ordinary compiler stack, not one per case.
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(hd_sched::pool::WORKER_STACK)
                .spawn_scoped(scope, || instrumented::run(cases, store, directory))
                .expect("spawn serial attribution worker")
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }
    #[cfg(not(coverage))]
    {
        let _ = (cases, store, directory);
        panic!("HD_COVERAGE_CASE_DIR requires an instrumented --cfg=coverage build");
    }
}

#[cfg(coverage)]
mod instrumented {
    use super::{Case, Path, Store, Verdict};
    use std::ffi::CString;
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    // LLVM compiler-rt's public instr_prof_interface.h, not a private profile ABI.
    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn __llvm_profile_set_filename(name: *const std::ffi::c_char);
        fn __llvm_profile_dump() -> std::ffi::c_int;
        fn __llvm_profile_reset_counters();
    }

    struct Collector {
        child: Child,
        input: ChildStdin,
        replies: Receiver<std::io::Result<String>>,
        directory: std::path::PathBuf,
        filename: Option<CString>,
    }

    impl Collector {
        fn new(directory: &Path) -> Self {
            std::fs::create_dir_all(directory).expect("create case profile directory");
            let converter = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../bench/coverage/case-attribution.ts");
            // cost: one persistent converter; no harness spawn or inventory scan per case.
            let mut child = Command::new("node")
                .arg(converter)
                .arg(std::env::current_exe().expect("instrumented test binary"))
                .arg(directory)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("start case profile converter");
            let input = child.stdin.take().expect("converter stdin");
            let output = child.stdout.take().expect("converter stdout");
            let (send, replies) = channel();
            // cost: one reader, allowing a bounded reply wait even if conversion stalls.
            std::thread::spawn(move || {
                for line in BufReader::new(output).lines() {
                    if send.send(line).is_err() {
                        break;
                    }
                }
            });
            Self {
                child,
                input,
                replies,
                directory: directory.to_owned(),
                filename: None,
            }
        }

        #[allow(unsafe_code)]
        fn write(&mut self, id: usize, path: &str, seconds: f64) {
            let file = self.directory.join(format!("{id}.profraw"));
            let filename = CString::new(file.as_os_str().as_encoded_bytes())
                .expect("profile path contains no NUL");
            // SAFETY: only this worker controls profiling, between completed
            // cases. Keep LLVM's borrowed filename alive until it is reset in Drop.
            let status = unsafe {
                __llvm_profile_set_filename(filename.as_ptr());
                __llvm_profile_dump()
            };
            self.filename = Some(filename);
            assert_eq!(status, 0, "dump case profile");
            // Case paths come from TSV fields, so contain neither tabs nor newlines.
            writeln!(self.input, "{id}\t{seconds}\t{path}").expect("request conversion");
            self.input.flush().expect("flush conversion request");
            let reply = self
                .replies
                .recv_timeout(Duration::from_mins(1))
                .expect("case profile converter must reply within 60s")
                .expect("read conversion reply");
            assert_eq!(reply, "ok", "case profile conversion failed");
        }
    }

    impl Drop for Collector {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            // SAFETY: release LLVM's borrowed filename before our CString drops.
            // dump() leaves its already-dumped flag set, suppressing exit rewrites.
            unsafe { __llvm_profile_set_filename(std::ptr::null()) };
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[allow(unsafe_code)]
    fn reset() {
        // SAFETY: serial attribution has no case executing at this boundary.
        unsafe { __llvm_profile_reset_counters() };
    }

    pub(super) fn run(cases: &[Case], store: &Store, directory: &Path) -> Vec<Verdict> {
        let mut collector = Collector::new(directory);
        collector.write(0, "<setup>", 0.0);
        let mut results = Vec::with_capacity(cases.len());
        for (serial, case) in cases.iter().enumerate() {
            reset();
            let start = Instant::now();
            let verdict = super::super::caught(|| super::super::run_case(case, store, serial));
            collector.write(serial + 1, &case.path, start.elapsed().as_secs_f64());
            results.push(verdict);
        }
        results
    }
}
