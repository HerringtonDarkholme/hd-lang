#![forbid(unsafe_code)]
//! `hd_testkit` (dev only): the determinism matrix, the edit-script fuzzer
//! and the differential harness (testing-the-compiler.md §8.1, §8.2). The
//! matrix runs one build under several serial orders and compares bytes;
//! the fuzzer and the harness are not written yet.

use hd_base::{NotImplemented, Stage, StageResult};
use hd_sched::SerialOrder;

/// The orders of the matrix that need no threads (§8.1).
pub const ORDERS: [SerialOrder; 4] = [SerialOrder::Fifo, SerialOrder::Priority, SerialOrder::Shuffled(1), SerialOrder::Shuffled(2)];

/// Runs `build` under each order; every run must produce equal bytes.
pub fn determinism_matrix(build: &dyn Fn(SerialOrder) -> Vec<u8>) -> Result<(), String> {
    let first = build(ORDERS[0]);
    for o in &ORDERS[1..] {
        if build(*o) != first {
            return Err(format!("output differs under {o:?}"));
        }
    }
    Ok(())
}

/// The edit-script fuzzer of §8.2.
pub fn edit_script_fuzz(_seed: u64) -> StageResult<()> {
    Err(NotImplemented::new(Stage::PackageResult, "edit-script fuzzer"))
}

#[cfg(test)]
mod tests {
    use super::determinism_matrix;
    use hd_driver::{MemStore, SourceFile, run};

    #[test]
    fn hello_is_deterministic_across_orders() {
        let files = [SourceFile { path: "main.hd".into(), text: "fn main():\n    println(42)\n".into() }];
        let build = |_| run(&mut MemStore::default(), &files, "pkg.main").wasm.expect("wasm");
        assert!(determinism_matrix(&build).is_ok());
        assert!(determinism_matrix(&|o| format!("{o:?}").into_bytes()).is_err());
    }
}
