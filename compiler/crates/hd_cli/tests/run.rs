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

/// Runs a sample: a directory is a package, run as `hd run` from inside
/// it; a file is a single-file program, run as `hd FILE`.
fn hd_run(cache_dir: &Path, target: &Path) -> String {
    let mut command = hd(cache_dir);
    if target.is_dir() {
        command.current_dir(target).arg("run");
    } else {
        command.arg(target);
    }
    let output = command.output().expect("run hd");
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

const CASES: [(&str, &str); 12] = [
    ("hello/hello.hd", "42\n"),
    ("arith/main.hd", "7\n9\n3\n55\n-1\n0\n1\n16\n-10\n"),
    ("data", "3\n-4\n7\n30\n"),
    ("generic", "1\n6\n5\n"),
    ("trait", "12\n13\n101\n7\n"),
    ("fib", "6765\n"),
    ("std_types", "42\n"),
    ("exit/main.hd", EXIT),
    ("init", INIT),
    ("suspend/main.hd", SUSPEND),
    ("defer/main.hd", DEFER),
    ("gaps", GAPS),
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
        .arg(samples().join("err_exit/main.hd"))
        .output()
        .expect("run hd");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "got 1\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "oops: too big\n");
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

/// Outside any package, `hd FILE` builds FILE alone: a broken sibling
/// in the same directory is not part of the program (cli.file.run).
#[test]
fn run_file_ignores_broken_siblings() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-sibling");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("good.hd");
    std::fs::write(&file, "println(\"fine\")\n").expect("write");
    std::fs::write(dir.join("broken.hd"), "fn broken( :\n").expect("write");
    assert_eq!(hd_run(&cache("hd-cache-sibling"), &file), "fine\n");
}

/// An `unsupported` error names the file and position of the construct,
/// not the sentinel span `:0..0`.
#[test]
fn unsupported_error_names_its_file() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-unsupported");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("spread.hd");
    std::fs::write(
        &file,
        "data Point:\n    x: i32\n    y: i32\n\nfn main() -> void:\n    p := Point { x: +1, y: +2 }\n    q := Point { ...p, x: +3 }\n    pass\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-unsupported"))
        .arg(&file)
        .output()
        .expect("run hd");
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("spread.hd:"), "{err}");
    assert!(err.contains("a data literal spread"), "{err}");
    assert!(!err.contains(":0..0"), "{err}");
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

/// A package `shop` with one executable (`main.hd`) and one task (`seed`).
fn shop(name: &str) -> PathBuf {
    let dir = scratch(name);
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("main.hd"),
        "fn main() -> void $ Console:\n    println(\"main\")\n",
    )
    .expect("write");
    std::fs::create_dir_all(dir.join("tasks")).expect("dir");
    std::fs::write(
        dir.join("tasks/seed.hd"),
        "fn main() -> void $ Console:\n    println(\"seeded\")\n",
    )
    .expect("write");
    dir
}

struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

fn ran(dir: &Path, args: &[&str]) -> Ran {
    let output = hd(&cache("hd-cache-forms"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run hd");
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// `cli.run.default.one`: `hd run` runs the package's one executable, from
/// a directory below the package too (`cli.mode.package.nearest`).
#[test]
fn run_alone_runs_the_one_executable() {
    let dir = shop("hd-forms-default");
    let r = ran(&dir, &["run"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "main\n"), "{}", r.err);
    let r = ran(&dir.join("tasks"), &["run"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "main\n"), "{}", r.err);
}

/// `cli.run.default.one`: a package with no executable is an error.
#[test]
fn run_alone_without_an_executable_is_an_error() {
    let dir = shop("hd-forms-library");
    std::fs::remove_file(dir.join("main.hd")).expect("remove");
    std::fs::write(dir.join("lib.hd"), "pub fn one() -> i32:\n    +1\n").expect("write");
    let r = ran(&dir, &["run"]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("no executable"), "{}", r.err);
}

/// `cli.run.name`: NAME is an executable (by the package's name) or a task,
/// and it is an error when there is none.
#[test]
fn run_name_runs_an_executable_or_a_task() {
    let dir = shop("hd-forms-name");
    let r = ran(&dir, &["run", "shop"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "main\n"), "{}", r.err);
    let r = ran(&dir, &["run", "seed"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "seeded\n"), "{}", r.err);
    let r = ran(&dir, &["run", "missing"]);
    assert_eq!(r.code, Some(101));
    assert!(
        r.err.contains("no executable or task named `missing`"),
        "{}",
        r.err
    );
}

/// `cli.run.file`: `hd run FILE` is an error that suggests `hd run` and
/// `hd run NAME`, in a package and outside one.
#[test]
fn run_file_is_an_error_that_suggests_run_and_name() {
    let dir = shop("hd-forms-run-file");
    for file in ["main.hd", "tasks/seed.hd"] {
        let r = ran(&dir, &["run", file]);
        assert_eq!(r.code, Some(101), "{file}");
        assert!(r.out.is_empty(), "{}", r.out);
        assert!(r.err.contains("`hd run`"), "{}", r.err);
        assert!(r.err.contains("`hd run NAME`"), "{}", r.err);
    }
    let outside = scratch("hd-forms-run-file-outside");
    std::fs::write(outside.join("a.hd"), "println(\"a\")\n").expect("write");
    let r = ran(&outside, &["run", "a.hd"]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("`hd run NAME`"), "{}", r.err);
}

/// `cli.command.positional`: a directory is neither NAME nor FILE.
#[test]
fn run_directory_suggests_the_package_flag() {
    let dir = shop("hd-forms-run-dir");
    let r = ran(&dir, &["run", "tasks/"]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("-p NAME"), "{}", r.err);
}

/// `cli.run.package-only`: outside any package, `hd run` and `hd build` are
/// errors that suggest `hd new`.
#[test]
fn run_and_build_outside_a_package_suggest_hd_new() {
    let outside = scratch("hd-forms-outside");
    std::fs::write(outside.join("a.hd"), "println(\"a\")\n").expect("write");
    for args in [&["run"][..], &["run", "a"], &["build"], &["build", "a.hd"]] {
        let r = ran(&outside, args);
        assert_eq!(r.code, Some(101), "{args:?}");
        assert!(r.err.contains("`hd new`"), "{args:?}: {}", r.err);
    }
}

/// `cli.file.run`: `hd FILE` runs a file of a package as a single file.
#[test]
fn bare_file_runs_inside_a_package() {
    let dir = shop("hd-forms-bare");
    let r = ran(&dir, &["tasks/seed.hd"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "seeded\n"), "{}", r.err);
}

/// `cli.build.output`: a whole-package `hd build` writes each executable
/// to `build/debug/NAME.wasm`, or `build/release/NAME.wasm`.
#[test]
fn build_writes_each_executable() {
    let dir = shop("hd-forms-build");
    let r = ran(&dir, &["build"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    let bytes = std::fs::read(dir.join("build/debug/shop.wasm")).expect("read wasm");
    assert_eq!(&bytes[..4], b"\0asm");
    let r = ran(&dir, &["build", "--release"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(dir.join("build/release/shop.wasm").is_file());
}

/// `cli.build.output.file`: `hd build FILE` writes
/// `build/debug/files/STEM.wasm`, or the release directory's.
#[test]
fn build_file_writes_under_files() {
    let dir = shop("hd-forms-build-file");
    let r = ran(&dir, &["build", "tasks/seed.hd"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    let bytes = std::fs::read(dir.join("build/debug/files/seed.wasm")).expect("read wasm");
    assert_eq!(&bytes[..4], b"\0asm");
    assert!(!dir.join("build/debug/shop.wasm").exists());
    let r = ran(&dir, &["build", "--release", "main.hd"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(dir.join("build/release/files/main.wasm").is_file());
}

/// `cli.build.library-only`: a package with no executable is checked and
/// writes no `.wasm` file.
#[test]
fn build_of_a_library_writes_no_module() {
    let dir = shop("hd-forms-build-lib");
    std::fs::remove_file(dir.join("main.hd")).expect("remove");
    std::fs::write(dir.join("lib.hd"), "pub fn one() -> i32:\n    +1\n").expect("write");
    let r = ran(&dir, &["build"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(!dir.join("build/debug").exists());
}

/// The removed `-o` form is a command-line error (`cli.exit.hd-failure`).
#[test]
fn build_rejects_the_output_flag() {
    let dir = shop("hd-forms-build-o");
    let r = ran(&dir, &["build", "main.hd", "-o", "x.wasm"]);
    assert_eq!(r.code, Some(101));
    assert!(!dir.join("x.wasm").exists());
}
