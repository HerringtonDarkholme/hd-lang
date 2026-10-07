#![forbid(unsafe_code)]
//! `hd_testkit` (dev only): the determinism matrix, the edit-script fuzzer
//! and the differential harness (testing-the-compiler.md §8.1, §8.2). The
//! matrix runs one build under several executors (serial orders and the
//! pool) and compares bytes; the fuzzer and the harness are not written yet.

use hd_base::{NotImplemented, Stage, StageResult};
use hd_driver::Executor;
use hd_sched::SerialOrder;

/// The executors of the matrix (§8.1): every serial order, shuffled with
/// several seeds, and the pool.
pub const EXECUTORS: [Executor; 7] = [
    Executor::Serial(SerialOrder::Fifo),
    Executor::Serial(SerialOrder::Priority),
    Executor::Serial(SerialOrder::Shuffled(1)),
    Executor::Serial(SerialOrder::Shuffled(2)),
    Executor::Serial(SerialOrder::Shuffled(0x5eed)),
    Executor::Pool(2),
    Executor::Pool(8),
];

/// Runs `build` under each executor; every run must produce equal bytes.
pub fn determinism_matrix(build: &dyn Fn(Executor) -> Vec<u8>) -> Result<(), String> {
    let first = build(EXECUTORS[0]);
    for e in &EXECUTORS[1..] {
        if build(*e) != first {
            return Err(format!("output differs under {e:?}"));
        }
    }
    Ok(())
}

/// The edit-script fuzzer of §8.2.
pub fn edit_script_fuzz(_seed: u64) -> StageResult<()> {
    Err(NotImplemented::new(
        Stage::PackageResult,
        "edit-script fuzzer",
    ))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::determinism_matrix;
    use hd_cache::MemoryStore;
    use hd_driver::{Executor, Goal, Host, NoClock, build};
    use hd_project::MemorySources;

    fn sample(dir: &str) -> MemorySources {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../samples")
            .join(dir);
        let mut s = MemorySources::default();
        let mut stack: Vec<PathBuf> = vec![root.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).expect("dir").flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "hd") {
                    let rel = p
                        .strip_prefix(&root)
                        .expect("root")
                        .to_string_lossy()
                        .replace('\\', "/");
                    s.insert(&rel, &std::fs::read_to_string(&p).expect("read"));
                }
            }
        }
        s
    }

    fn wasm(src: &MemorySources, entry: &str, executor: Executor) -> Vec<u8> {
        let store = MemoryStore::default();
        let host = Host {
            render_tir: &[],
            sources: src,
            store: &store,
            clock: &NoClock,
            executor,
        };
        let out = build(
            &host,
            "app",
            &Goal::Program {
                entry: entry.into(),
            },
        );
        assert!(out.diags.is_empty(), "{}", out.render());
        out.wasm.expect("wasm")
    }

    #[test]
    fn every_sample_is_byte_identical_under_every_executor() {
        for (dir, entry) in [
            ("hello", "hello"),
            ("arith", "main"),
            ("data", "main"),
            ("generic", "main"),
            ("trait", "main"),
            ("fib", "main"),
        ] {
            let src = sample(dir);
            assert!(
                determinism_matrix(&|e| wasm(&src, entry, e)).is_ok(),
                "{dir}"
            );
        }
        assert!(determinism_matrix(&|e| format!("{e:?}").into_bytes()).is_err());
    }
}
