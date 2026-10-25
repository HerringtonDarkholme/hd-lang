//! A named pipe inside a package is no source and no hashed input (#281):
//! package discovery, loading and the `--affected` fingerprints open only
//! regular files, since opening a FIFO with no writer blocks forever.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use hd_testkit::Scratch;

const MAIN: &str = "use std.testing.assert_equal\n\npub fn main() -> void $ Console:\n    println(\"hi\")\n\n\
                    tests:\n    it(\"adds\"):\n        assert_equal(1 + 1, 2, reason=\"sum\")\n";

fn fifo(path: &Path) {
    let made = Command::new("mkfifo").arg(path).status().expect("mkfifo");
    assert!(made.success(), "mkfifo {}", path.display());
}

/// A package with a FIFO in its root and one in `src/`, one named like a
/// source, one in `tests/`.
fn package(name: &str) -> Scratch {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::create_dir_all(root.join("tests")).expect("tests");
    std::fs::write(
        root.join("hd.toml"),
        "[package]\nname = \"pipes\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("hd.toml");
    std::fs::write(root.join("src/main.hd"), MAIN).expect("main");
    std::fs::write(
        root.join("tests/it.hd"),
        "use std.testing.assert_equal\n\nit(\"ok\"):\n    assert_equal(1, 1, reason=\"one\")\n",
    )
    .expect("it");
    fifo(&root.join("pipe"));
    fifo(&root.join("src/pipe"));
    fifo(&root.join("src/named.hd"));
    fifo(&root.join("tests/pipe"));
    root
}

/// Runs `hd ARGS` and returns its exit status, or fails the test when it
/// does not finish in time (a regression hangs).
fn hd(root: &Path, args: &[&str]) -> std::process::ExitStatus {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", root.with_extension("cache"))
        .env_remove("HD_JOBS")
        .env_remove("HD_ENGINE")
        .current_dir(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("run hd");
    let deadline = Instant::now() + Duration::from_mins(2);
    loop {
        if let Some(status) = child.try_wait().expect("wait") {
            return status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("`hd {}` hung on a FIFO", args.join(" "));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn check_finishes() {
    let root = package("fifo_check");
    assert!(hd(&root, &["check", "--all"]).success());
}

#[test]
fn test_finishes() {
    let root = package("fifo_test");
    assert!(hd(&root, &["test"]).success());
}

#[test]
fn build_finishes() {
    let root = package("fifo_build");
    assert!(hd(&root, &["build"]).success());
}

#[test]
fn fmt_check_finishes() {
    let root = package("fifo_fmt");
    assert!(hd(&root, &["fmt", "--check"]).success());
}

#[test]
fn test_affected_finishes() {
    let root = package("fifo_affected");
    assert!(hd(&root, &["test"]).success());
    assert!(hd(&root, &["test", "--affected"]).success());
}
