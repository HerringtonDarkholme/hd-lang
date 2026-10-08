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

/// The phase-1 exit program (build-order.md, M4b): `List`, `Map`,
/// `Option`, `Result`, strings with interpolation, a user trait, a
/// closure, `match`, `for` and std's own `println`.
const EXIT: &str = "total 14 of 5\n40\nrect 6\ncircle 12\nfound 4\nmissing\nok 3\n\
                    err divide by zero\nnum 7\nplus\nword hi\nann 31, cy 40\n31 years\n3\n";

/// M4d: module initialization across two modules. `prices` initializes
/// before `main`'s top level, which reads its `markup`, and both run
/// before `main`.
const INIT: &str = "hello\n15\n1\n5\n2\n";

/// M4d: `main!` awaits a host timer, then two timed operations with
/// `all!`, then a `race!` whose loser is cancelled mid-wait: its `defer`
/// runs and its end never does.
const SUSPEND: &str = "slept\nall 10 20\nrace 30\nstart 1\nstart 2\ndone 2\ncleanup 2\n\
                       done 1\ncleanup 1\nstart 3\nstart 4\ndone 3\ncleanup 3\ncleanup 4\n";

/// M4d: `defer` ordering on each exit kind: falling off the end (last
/// in, first out), `return`, `continue`, `break`, a failing `?` and a
/// nested scope's `return`.
const DEFER: &str = "5\nerr bad\nok 2\n7\nnormal: body\nnormal: second registered\n\
                     normal: first registered\nreturn: cleanup 5\nloop: cleanup 1\n\
                     loop: cleanup 2\nloop: after\ntry: cleanup x\ntry: cleanup y\n\
                     nested: inner\nnested: outer\n";

/// M4c: shared enum data (read across modules), `$x` of a top-level
/// binding, and `for` over ranges and a list.
const GAPS: &str = "200 OK false\n404 Not Found false\n503 Service Unavailable true\n\
                    level 1 top 9\nhi\n0\n1\n2\n25\ntick\ntick\na\nb\n";

const CASES: [(&str, &str); 13] = [
    ("hello/hello.hd", "42\n"),
    ("hello", "42\n"),
    ("arith", "7\n9\n3\n55\n-1\n0\n1\n16\n-10\n"),
    ("data", "3\n-4\n7\n30\n"),
    ("generic", "1\n6\n5\n"),
    ("trait/main.hd", "12\n13\n101\n7\n"),
    ("fib/main.hd", "6765\n"),
    ("std_types", "42\n"),
    ("exit", EXIT),
    ("init/main.hd", INIT),
    ("suspend/main.hd", SUSPEND),
    ("defer/main.hd", DEFER),
    ("gaps/main.hd", GAPS),
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

/// M4c: `main` returning `.Err` prints the error's report on standard
/// error and exits with status 1 (module.entry.err-stderr).
#[test]
fn main_returning_err_exits_with_its_report() {
    let output = hd(&cache("hd-cache-err-exit"))
        .arg("run")
        .arg(samples().join("err_exit/main.hd"))
        .output()
        .expect("run hd");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "got 1\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "oops: too big\n");
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
    std::fs::write(
        &file,
        "fn main() -> void $ Console:\n    println(missing)\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-error"))
        .arg("run")
        .arg(&file)
        .output()
        .expect("run hd");
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("unknown-name `missing`"), "{err}");
}

/// A script is an entry module with top-level statements and no `main`:
/// its statements are the whole program (module.init.script).
#[test]
fn script_runs_its_top_level_statements() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-script");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("hello.hd");
    std::fs::write(&file, "x := 40 + 2\nprintln(x)\n").expect("write");
    assert_eq!(hd_run(&cache("hd-cache-script"), &file), "42\n");
}
