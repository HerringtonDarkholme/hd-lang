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
    assert!(
        err.contains("unknown-name: `missing` is not defined"),
        "{err}"
    );
}

/// A rendered line is `severity: file:line:column: code: message`; the
/// message must not repeat the code.
#[test]
fn rendered_diagnostics_do_not_repeat_their_code() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-no-repeat");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("twice.hd");
    std::fs::write(
        &file,
        "fn main() -> void $ Console:\n    println(missing)\n    let flag: i32 = true\n    println(flag)\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-no-repeat"))
        .arg(&file)
        .output()
        .expect("run hd");
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<&str> = err.lines().collect();
    assert!(lines.len() >= 2, "{err}");
    for line in lines {
        let parts: Vec<&str> = line.splitn(4, ": ").collect();
        assert_eq!(parts.len(), 4, "{line}");
        assert!(!parts[3].contains(parts[2]), "{line}");
    }
}

/// `cli.diagnostics.text`: each diagnostic prints its severity, then
/// `file:line:column`, the code once, and the message.
#[test]
fn warning_and_error_print_severity_location_code_and_message() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-severity");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("mixed.hd");
    std::fs::write(
        &file,
        "fn add(a: i32, b: i32) -> i32:\n    a\n\nfn main() -> void $ Console:\n    u := +1\n    x := add(+1)\n    println(x)\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-severity"))
        .arg(&file)
        .output()
        .expect("run hd");
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        err,
        "warning: mixed.hd:5:5: unused-local-binding: `u` is never read; name it `_u` to keep it\n\
         error: mixed.hd:6:10: argument-count: `add` takes 2 arguments\n"
    );
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
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("write");
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

/// `s[i]` and slices read a string as a view over shared bytes: a slice of
/// a non-ASCII string indexes from its own start, and an access outside
/// the view panics with `index-out-of-bounds` at the call site.
#[test]
fn string_index_and_slice_on_views() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-str-index");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("views.hd");
    std::fs::write(
        &file,
        "fn main() -> void $ Console:\n\
         \x20   s := \"h\u{e9}llo w\u{f6}rld\"\n\
         \x20   println(\"${s.len()} ${s[0]} ${s[1]}\")\n\
         \x20   word := s.slice(7, 12)\n\
         \x20   println(word)\n\
         \x20   println(\"${word.len()} ${word[0]} ${word[2]}\")\n\
         \x20   println(word.slice(0, 1))\n\
         \x20   println(word.slice(1, 3))\n\
         \x20   let lo: usize = 7\n\
         \x20   let hi: usize = 10\n\
         \x20   println(s[lo..hi])\n\
         \x20   println(s[lo..])\n\
         \x20   println(word[5])\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-str-index"))
        .arg(&file)
        .output()
        .expect("run hd");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "13 104 195\nw\u{f6}rl\n5 119 182\nw\n\u{f6}\nw\u{f6}\nw\u{f6}rld\n"
    );
    assert_eq!(output.status.code(), Some(3));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("panic: index-out-of-bounds"), "{err}");
}

/// A slice that is not on scalar boundaries, or is reversed, panics.
#[test]
fn string_slice_off_boundary_panics() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-str-slice");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("bad.hd");
    std::fs::write(
        &file,
        "fn main() -> void $ Console:\n\
         \x20   s := \"h\u{e9}llo\"\n\
         \x20   println(s.slice(1, 3))\n\
         \x20   println(s.slice(2, 3))\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-str-slice"))
        .arg(&file)
        .output()
        .expect("run hd");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "\u{e9}\n");
    assert_eq!(output.status.code(), Some(3));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("panic: index-out-of-bounds"), "{err}");
}

/// A closure that assigns a captured `let` shares one cell with its owner:
/// every call sees the previous call's write, and the owner reads the final
/// value afterwards.
#[test]
fn closure_mutating_a_captured_let_shares_a_cell() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-shared-cell");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("counter.hd");
    std::fs::write(
        &file,
        "fn main() -> void $ Console:\n\
         \x20   let n: i32 = +0\n\
         \x20   bump := fn() -> i32:\n\
         \x20       n = n + 1\n\
         \x20       return n\n\
         \x20   println(\"${bump()} ${bump()} ${bump()}\")\n\
         \x20   println(\"${n}\")\n\
         \x20   n = n + 10\n\
         \x20   println(\"${bump()}\")\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-shared-cell"))
        .arg(&file)
        .output()
        .expect("run hd");
    let err = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "1 2 3\n3\n14\n",
        "{err}"
    );
}

/// A cell outlives the function that made it, and a `let` inside a
/// loop gets a fresh cell per iteration.
#[test]
fn shared_cells_outlive_their_function_and_renew_per_iteration() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-shared-cell-more");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("more.hd");
    std::fs::write(
        &file,
        "fn counter(start: i32) -> fn() -> i32:\n\
         \x20   let at = start\n\
         \x20   return fn() -> i32:\n\
         \x20       at = at + 1\n\
         \x20       return at\n\
         fn main() -> void $ Console:\n\
         \x20   a := counter(+10)\n\
         \x20   b := counter(+20)\n\
         \x20   println(\"${a()} ${a()} ${b()}\")\n\
         \x20   let i: i32 = +0\n\
         \x20   while i < 2:\n\
         \x20       let n: i32 = i * 100\n\
         \x20       bump := fn() -> i32:\n\
         \x20           n = n + 1\n\
         \x20           return n\n\
         \x20       println(\"${bump()} ${bump()}\")\n\
         \x20       i = i + 1\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-shared-cell-more"))
        .arg(&file)
        .output()
        .expect("run hd");
    let err = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "11 12 21\n1 2\n101 102\n",
        "{err}"
    );
}

/// `chars` is built on a mutably capturing closure (`Iterator::from_fn`).
#[test]
fn chars_walks_a_non_ascii_string() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-chars");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("chars.hd");
    std::fs::write(
        &file,
        "fn main() -> void $ Console:\n\
         \x20   for c in \"h\u{e9}llo\".chars():\n\
         \x20       println(\"${c}\")\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-chars"))
        .arg(&file)
        .output()
        .expect("run hd");
    let err = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "h\n\u{e9}\nl\nl\no\n",
        "{err}"
    );
}

/// `bytes` is another `Iterator::from_fn` over a captured, assigned offset.
#[test]
fn bytes_walks_a_non_ascii_string() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-bytes");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("bytes.hd");
    std::fs::write(
        &file,
        "fn main() -> void $ Console:\n\
         \x20   for b in \"h\u{e9}y\".bytes():\n\
         \x20       println(\"${b}\")\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-bytes"))
        .arg(&file)
        .output()
        .expect("run hd");
    let err = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "104\n195\n169\n121\n",
        "{err}"
    );
}

/// `join` concatenates a list of strings through `bytes_concat`.
#[test]
fn join_list_of_strings() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-join");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("join.hd");
    std::fs::write(
        &file,
        "fn main() -> void $ Console:\n\
         \x20   let parts: List[string] = [\"ab\", \"c\u{e9}\", \"d\"]\n\
         \x20   println(parts.join(\"-\"))\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-join"))
        .arg(&file)
        .output()
        .expect("run hd");
    let err = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "ab-c\u{e9}-d\n",
        "{err}"
    );
}

/// `to_utf8` builds a byte list and `string::from_utf8` builds the string
/// back through `string_from_bytes`.
#[test]
fn utf8_bytes_round_trip() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-run-utf8");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("utf8.hd");
    std::fs::write(
        &file,
        "fn main() -> void $ Console:\n\
         \x20   let bytes = \"h\u{e9}llo\".to_utf8()\n\
         \x20   println(\"${bytes.len()}\")\n\
         \x20   match string::from_utf8(bytes):\n\
         \x20       .Ok(text) => println(text)\n\
         \x20       .Err(_) => println(\"invalid\")\n",
    )
    .expect("write");
    let output = hd(&cache("hd-cache-utf8"))
        .arg(&file)
        .output()
        .expect("run hd");
    let err = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "6\nh\u{e9}llo\n",
        "{err}"
    );
}

/// `cli.json.run`, `cli.json.run.program`: with `--format json`, `hd run`
/// and `hd FILE` write the program's output to stdout untouched, and only
/// their own JSON lines, ending in the summary, to stderr.
#[test]
fn run_and_file_write_json_to_stderr_and_the_program_to_stdout() {
    let dir = shop("hd-forms-json-run");
    let summary = "{\"kind\":\"summary\",\"errors\":0,\"warnings\":0,\"passed\":0,\"failed\":0,\"ignored\":0,\"status\":0}\n";
    for args in [
        &["run", "--format", "json"][..],
        &["--format=json", "main.hd"][..],
        &["main.hd", "--format", "json"][..],
    ] {
        let r = ran(&dir, args);
        assert_eq!((r.code, r.out.as_str()), (Some(0), "main\n"), "{args:?}");
        assert_eq!(r.err, summary, "{args:?}");
    }
    std::fs::write(
        dir.join("main.hd"),
        "fn main() -> void $ Console:\n    println(missing)\n",
    )
    .expect("write");
    let r = ran(&dir, &["run", "--format", "json"]);
    assert_eq!(r.code, Some(101));
    assert_eq!(r.out, "");
    let lines: Vec<&str> = r.err.lines().collect();
    assert_eq!(lines.len(), 2, "{}", r.err);
    assert!(
        lines[0].contains("\"code\":\"unknown-name\"")
            && lines[0].contains("\"file\":\"main.hd\",\"line\":2,\"column\":13"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].contains("\"errors\":1") && lines[1].contains("\"status\":101"),
        "{}",
        lines[1]
    );
}

/// `cli.json.lines.build`: `hd build --format json` writes JSON lines to
/// stdout, the summary on success too.
#[test]
fn build_writes_json_lines_to_stdout() {
    let dir = shop("hd-forms-json-build");
    let r = ran(&dir, &["build", "--format", "json"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(r.err, "");
    assert_eq!(
        r.out,
        "{\"kind\":\"summary\",\"errors\":0,\"warnings\":0,\"passed\":0,\"failed\":0,\"ignored\":0,\"status\":0}\n"
    );
    assert!(dir.join("build/debug/shop.wasm").is_file());
    let r = ran(&dir, &["build", "--format", "yaml"]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("`--format`"), "{}", r.err);
}

/// `cli.cap.total.refuse`, `cli.cap.total.message`, `cli.cap.order.deny`:
/// a totally denied need stops the program before it starts, with status
/// 101 and a message that names the trait and the setting.
#[test]
fn a_totally_denied_need_refuses_to_start() {
    let dir = shop("hd-forms-deny");
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = false\n",
    )
    .expect("write");
    for args in [&["run"][..], &["run", "--cap", "Console=true"][..]] {
        let r = ran(&dir, args);
        assert_eq!((r.code, r.out.as_str()), (Some(101), ""), "{args:?}");
        assert_eq!(
            r.err,
            "error: denied-capability: the program needs Console, which `Console = false` in hd.toml denies, so it does not start\n",
            "{args:?}"
        );
    }
    let alone = scratch("hd-forms-deny-file");
    std::fs::write(
        alone.join("hi.hd"),
        "fn main() -> void $ Console:\n    println(\"hi\")\n",
    )
    .expect("write");
    let r = ran(&alone, &["--cap", "Console=false", "hi.hd"]);
    assert_eq!((r.code, r.out.as_str()), (Some(101), ""));
    assert!(r.err.contains("`--cap Console=false` denies"), "{}", r.err);
    let r = ran(&alone, &["--cap=Http=example.com", "hi.hd"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "hi\n"), "{}", r.err);
}

/// `cli.cap.flag.unknown`, `cli.cap.flag.unscoped`, `cli.cap.table.keys`.
#[test]
fn a_grant_must_name_a_host_trait_and_fit_its_kind() {
    let dir = shop("hd-forms-bad-cap");
    for flag in ["Time=true", "Console=a,b"] {
        let r = ran(&dir, &["run", "--cap", flag]);
        assert_eq!(r.code, Some(101), "{flag}");
        assert!(r.err.contains(flag), "{flag}: {}", r.err);
    }
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nTime = true\n",
    )
    .expect("write");
    let r = ran(&dir, &["run"]);
    assert_eq!(r.code, Some(101));
    assert!(
        r.err
            .starts_with("error: hd.toml:5:1: `Time` is no host capability trait"),
        "{}",
        r.err
    );
}

/// Writes `files` under a fresh directory.
fn tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = scratch(name);
    for (file, text) in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }
    dir
}

const HELLO: &str = "pub fn main() -> void $ Console:\n    println(\"hello\")\n";

/// `cli.exe.table`, `cli.exe.other-module`: an `[[executable]]` table names
/// its entry module below the source root, and `hd run NAME` runs it.
#[test]
fn executable_tables_name_their_entry_modules() {
    let dir = tree(
        "hd-forms-exe",
        &[
            (
                "hd.toml",
                "[package]\nname = \"shop\"\n\n[[executable]]\nname = \"migrate\"\nmodule = \"tools.migrate\"\n",
            ),
            (
                "src/tools/migrate.hd",
                "pub fn main() -> void $ Console:\n    println(\"migrated\")\n",
            ),
        ],
    );
    let r = ran(&dir, &["run", "migrate"]);
    assert_eq!(
        (r.code, r.out.as_str()),
        (Some(0), "migrated\n"),
        "{}",
        r.err
    );
    let r = ran(&dir, &["build"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(dir.join("build/debug/migrate.wasm").is_file());
}

/// `cli.workspace.members`, `cli.workspace.run-name`, `.run-bare`,
/// `cli.mode.member.unlisted`: at a workspace root, check and build act on
/// every member, and `hd run NAME` finds the one member with NAME; a
/// package the manifest does not list is an error naming the manifest.
#[test]
fn a_workspace_root_acts_on_its_members() {
    let dir = tree(
        "hd-forms-workspace",
        &[
            ("hd.toml", "[workspace]\nmembers = [\"app\", \"lib\"]\n"),
            ("app/hd.toml", "[package]\nname = \"app\"\n"),
            ("app/src/main.hd", HELLO),
            ("lib/hd.toml", "[package]\nname = \"lib\"\n"),
            ("lib/src/lib.hd", "pub fn one() -> i32:\n    missing\n"),
            ("loose/hd.toml", "[package]\nname = \"loose\"\n"),
            ("loose/src/main.hd", HELLO),
        ],
    );
    let r = ran(&dir, &["check"]);
    assert_eq!(r.code, Some(101));
    assert!(
        r.err.starts_with("error: lib/src/lib.hd:2:5: unknown-name"),
        "{}",
        r.err
    );
    std::fs::write(dir.join("lib/src/lib.hd"), "pub fn one() -> i32:\n    1\n").expect("write");
    let r = ran(&dir, &["build"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(dir.join("app/build/debug/app.wasm").is_file());
    let r = ran(&dir, &["run", "app"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "hello\n"), "{}", r.err);
    let r = ran(&dir, &["run"]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("app: app"), "{}", r.err);
    let r = ran(&dir.join("loose"), &["run"]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("neither lists `loose`"), "{}", r.err);
}

/// `module.test.code`: `hd run` and `hd build` build the executable without
/// the package's test code or its other tasks.
#[test]
fn a_program_leaves_test_code_out() {
    let dir = shop("hd-forms-no-tests");
    std::fs::create_dir_all(dir.join("tests")).expect("dir");
    std::fs::write(
        dir.join("tests/broken.hd"),
        "fn main() -> void:\n    _ := missing\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("tasks/broken.hd"),
        "fn main() -> void:\n    _ := missing\n",
    )
    .expect("write");
    let r = ran(&dir, &["run"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "main\n"), "{}", r.err);
    let r = ran(&dir, &["run", "seed"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "seeded\n"), "{}", r.err);
    let r = ran(&dir, &["build"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
}

/// `cli.command.help`, `cli.command.help.command`: the command list names
/// every command, and one command's help names its flags.
#[test]
fn help_lists_the_commands_and_each_commands_flags() {
    let dir = scratch("hd-forms-help");
    for args in [&["help"][..], &["--help"][..]] {
        let r = ran(&dir, args);
        assert_eq!(r.code, Some(0), "{}", r.err);
        for name in [
            "FILE.wasm",
            "build",
            "run",
            "test",
            "check",
            "doc",
            "new",
            "add",
            "update",
            "remove",
            "fetch",
            "clean",
            "fmt",
            "fix",
            "cache gc",
            "help",
        ] {
            assert!(r.out.contains(&format!("\n  {name} ")), "{name}: {}", r.out);
        }
    }
    let add = ran(&dir, &["help", "add"]);
    assert_eq!(add.code, Some(0));
    assert!(add.out.contains("--dev"), "{}", add.out);
    assert_eq!(ran(&dir, &["add", "--help"]).out, add.out);
    assert!(ran(&dir, &["clean", "--help"]).out.contains("--cache"));
    assert_eq!(ran(&dir, &["help", "bogus"]).code, Some(101));
}

/// `cli.profile.release.commands`, `cli.profile.test.release`: `hd FILE`
/// and `hd test` take `--release`, before FILE too; a test build stays
/// checked, so overflow still panics.
#[test]
fn release_comes_before_file_and_keeps_tests_checked() {
    let dir = scratch("hd-forms-release");
    std::fs::write(
        dir.join("main.hd"),
        "fn next(value: i32) -> i32:\n    value + 1\n\npub fn main() -> void $ Console:\n    println(next(1))\n\ntests:\n    use std.testing.assert_equal\n\n    it(\"overflows\"):\n        assert_equal(next(2147483647), 0, reason=\"panics first\")\n",
    )
    .expect("write");
    let r = ran(&dir, &["--release", "main.hd"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "2\n"), "{}", r.err);
    let r = ran(&dir, &["test", "--release", "main.hd"]);
    assert_eq!(r.code, Some(1), "{}", r.out);
    assert!(r.out.contains("integer-overflow"), "{}", r.out);
}

/// `cli.new.app`, `cli.new.workspace-member`, `cli.new.existing`: a new
/// application under a workspace root runs at once and joins `members`;
/// a second `hd new` over it writes nothing.
#[test]
fn new_creates_a_package_and_joins_its_workspace() {
    let dir = tree(
        "hd-forms-new",
        &[("hd.toml", "[workspace]\nmembers = [\"old\"]\n")],
    );
    let r = ran(&dir, &["new", "--app", "--vcs", "none", "my-app"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(
        std::fs::read_to_string(dir.join("hd.toml")).expect("read"),
        "[workspace]\nmembers = [\"old\", \"my-app\"]\n"
    );
    assert!(dir.join("my-app/tests/my_app.hd").is_file());
    let r = ran(&dir.join("my-app"), &["run"]);
    assert_eq!(
        (r.code, r.out.as_str()),
        (Some(0), "hello, world\n"),
        "{}",
        r.err
    );
    let r = ran(&dir, &["new", "--lib", "--vcs", "none", "my-app"]);
    assert_eq!(r.code, Some(101));
    assert!(!dir.join("my-app/src/lib.hd").exists());
}

/// `cli.clean.cache.*`: `hd clean --cache` removes the read-only fetched
/// versions and the compiled entries, and keeps the cache directory; a
/// directory that holds anything else is no cache, and nothing goes.
#[test]
fn clean_cache_empties_only_an_hd_cache() {
    let dir = scratch("hd-forms-clean-cache");
    let cache = dir.join("cache");
    let version = cache.join("pkg/github.com/acme/json@2.1.0");
    std::fs::create_dir_all(version.join("src")).expect("dir");
    std::fs::write(version.join("hd.toml"), "[package]\nname = \"json\"\n").expect("write");
    std::fs::create_dir_all(cache.join("obj/check")).expect("dir");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for d in [version.join("src"), version.clone()] {
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o555)).expect("chmod");
        }
    }
    let clean = |cache: &Path| {
        hd(cache)
            .current_dir(&dir)
            .args(["clean", "--cache"])
            .output()
            .expect("run hd")
    };
    let out = clean(&cache);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("removed github.com/acme/json@2.1.0\nremoved 1 version from "),
        "{text}"
    );
    assert!(cache.is_dir() && !cache.join("pkg").exists() && !cache.join("obj").exists());
    let again = clean(&cache);
    assert!(String::from_utf8_lossy(&again.stdout).contains("is empty"));
    std::fs::write(dir.join("notes.txt"), "mine").expect("write");
    let foreign = clean(&dir);
    assert_eq!(foreign.status.code(), Some(101));
    assert!(dir.join("notes.txt").is_file());
}
