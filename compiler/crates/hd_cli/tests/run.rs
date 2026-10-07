//! `hd run` and `hd build` on the samples in `compiler/samples`, on V8.

use std::path::{Path, PathBuf};
use std::process::Command;

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples")
}

fn hd_run(target: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_hd"))
        .arg("run")
        .arg(target)
        .output()
        .expect("run hd");
    assert!(output.status.success(), "{}: {}", target.display(), String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).expect("UTF-8")
}

#[test]
fn run_samples_on_v8() {
    let cases: [(&str, &str); 7] = [
        ("hello/hello.hd", "42\n"),
        ("hello", "42\n"),
        ("arith", "7\n9\n3\n55\n-1\n0\n1\n16\n-10\n"),
        ("data", "3\n-4\n7\n30\n"),
        ("generic", "1\n6\n5\n"),
        ("trait/main.hd", "12\n13\n101\n7\n"),
        ("fib/main.hd", "6765\n"),
    ];
    for (target, expected) in cases {
        assert_eq!(hd_run(&samples().join(target)), expected, "{target}");
    }
}

#[test]
fn build_writes_a_module() {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-build-fib.wasm");
    let status = Command::new(env!("CARGO_BIN_EXE_hd"))
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
    let output = Command::new(env!("CARGO_BIN_EXE_hd")).arg("run").arg(&file).output().expect("run hd");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown-name `missing`"));
}
