//! `hd run` and `hd build` on the samples in `compiler/samples`, on V8,
//! with the disk cache in a test directory: a second run is warm.

use std::path::{Path, PathBuf};
use std::process::Command;

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples")
}

fn cache(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

fn hd(cache_dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_hd"));
    c.env("HD_CACHE", cache_dir);
    c
}

fn hd_run(cache_dir: &Path, target: &Path) -> String {
    let output = hd(cache_dir)
        .arg("run")
        .arg(target)
        .output()
        .expect("run hd");
    assert!(
        output.status.success(),
        "{}: {}",
        target.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8")
}

const CASES: [(&str, &str); 8] = [
    ("hello/hello.hd", "42\n"),
    ("hello", "42\n"),
    ("arith", "7\n9\n3\n55\n-1\n0\n1\n16\n-10\n"),
    ("data", "3\n-4\n7\n30\n"),
    ("generic", "1\n6\n5\n"),
    ("trait/main.hd", "12\n13\n101\n7\n"),
    ("fib/main.hd", "6765\n"),
    ("std_types", "42\n"),
];

#[test]
fn run_samples_on_v8_cold_then_warm() {
    let dir = cache("hd-cache-samples");
    let _ = std::fs::remove_dir_all(&dir);
    for round in ["cold", "warm"] {
        for (target, expected) in CASES {
            assert_eq!(
                hd_run(&dir, &samples().join(target)),
                expected,
                "{target} ({round})"
            );
        }
    }
    assert!(
        dir.join("obj").join("link").is_dir(),
        "the disk cache holds linked programs"
    );
}

#[test]
fn build_writes_a_module() {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-build-fib.wasm");
    let status = hd(&cache("hd-cache-build"))
        .arg("build")
        .arg(samples().join("fib/main.hd"))
        .arg("-o")
        .arg(&out)
        .status()
        .expect("run hd build");
    assert!(status.success());
    let bytes = std::fs::read(&out).expect("read wasm");
    assert_eq!(&bytes[..4], b"\0asm");
}

#[test]
fn run_reports_check_errors() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-error");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("bad.hd");
    std::fs::write(&file, "fn main():\n    println(missing)\n").expect("write");
    let output = hd(&cache("hd-cache-error"))
        .arg("run")
        .arg(&file)
        .output()
        .expect("run hd");
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("unknown-name `missing`"), "{err}");
}
